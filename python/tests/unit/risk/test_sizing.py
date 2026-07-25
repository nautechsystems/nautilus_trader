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

from decimal import Decimal

import pytest

from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.risk import FixedRiskSizer
from nautilus_trader.risk import PositionSizer
from tests.providers import TestInstrumentProvider


GBPUSD = TestInstrumentProvider.default_fx_ccy("GBP/USD")
ENTRY = Price.from_str("1.00100")
STOP = Price.from_str("1.00000")


def test_position_sizer_exposes_and_updates_instrument():
    sizer = PositionSizer(GBPUSD)

    assert sizer.instrument == GBPUSD

    sizer.update_instrument(GBPUSD)

    assert sizer.instrument == GBPUSD


def test_calculate_with_zero_equity_returns_quantity_zero():
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP,
        equity=Money(0, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
    )

    assert result == Quantity.zero()


def test_calculate_with_zero_exchange_rate_returns_quantity_zero():
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP,
        equity=Money(1_000_000, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
        exchange_rate=Decimal(0),
    )

    assert result == Quantity.zero()


def test_calculate_with_zero_risk_distance_returns_quantity_zero():
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=ENTRY,
        equity=Money(1_000_000, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
    )

    assert result == Quantity.zero()


def test_calculate_single_unit_size():
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP,
        equity=Money(1_000_000, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
        unit_batch_size=Decimal(1000),
    )

    assert result == Quantity.from_str("1000000")


def test_calculate_hard_limit_caps_size():
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP,
        equity=Money(1_000_000, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
        unit_batch_size=Decimal(1000),
        hard_limit=Decimal(500_000),
    )

    assert result == Quantity.from_str("500000")


def test_calculate_defaults_match_explicit_defaults():
    sizer = FixedRiskSizer(GBPUSD)
    kwargs = {
        "entry": ENTRY,
        "stop_loss": STOP,
        "equity": Money(1_000_000, GBPUSD.quote_currency),
        "risk": Decimal("0.001"),
    }

    with_defaults = sizer.calculate(**kwargs)
    explicit = sizer.calculate(
        **kwargs,
        commission_rate=Decimal(0),
        exchange_rate=Decimal(1),
        hard_limit=None,
        unit_batch_size=Decimal(1),
        units=1,
    )

    assert with_defaults == explicit


def test_calculate_splits_size_across_units():
    sizer = FixedRiskSizer(GBPUSD)
    common = {
        "entry": ENTRY,
        "stop_loss": STOP,
        "equity": Money(1_000_000, GBPUSD.quote_currency),
        "risk": Decimal("0.001"),
        "unit_batch_size": Decimal(1000),
    }

    single = sizer.calculate(**common, units=1)
    triple = sizer.calculate(**common, units=3)

    assert triple > Quantity.zero()
    assert triple < single


@pytest.mark.parametrize("risk", [Decimal(0), Decimal("-0.001")])
def test_calculate_rejects_non_positive_risk(risk):
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="risk"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=risk,
        )


def test_fixed_risk_sizer_is_position_sizer():
    sizer = FixedRiskSizer(GBPUSD)

    assert isinstance(sizer, PositionSizer)


def test_fixed_risk_sizer_inherits_update_instrument():
    sizer = FixedRiskSizer(GBPUSD)

    assert sizer.instrument == GBPUSD
    sizer.update_instrument(GBPUSD)

    assert sizer.instrument == GBPUSD


def test_update_instrument_rejects_mismatched_id():
    sizer = PositionSizer(GBPUSD)
    other = TestInstrumentProvider.default_fx_ccy("USD/JPY")

    with pytest.raises(ValueError, match=r"instrument\.id"):
        sizer.update_instrument(other)


def test_calculate_rejects_negative_commission_rate():
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="commission_rate"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
            commission_rate=Decimal("-0.001"),
        )


def test_calculate_rejects_zero_units():
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="units"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
            units=0,
        )


@pytest.mark.parametrize("exchange_rate", [Decimal("-0.5"), Decimal(-1)])
def test_calculate_rejects_negative_exchange_rate(exchange_rate):
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="exchange_rate"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
            exchange_rate=exchange_rate,
        )


@pytest.mark.parametrize("hard_limit", [Decimal(0), Decimal(-100)])
def test_calculate_rejects_non_positive_hard_limit(hard_limit):
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="hard_limit"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
            hard_limit=hard_limit,
        )


@pytest.mark.parametrize("unit_batch_size", [Decimal(-1), Decimal(-1000)])
def test_calculate_rejects_negative_unit_batch_size(unit_batch_size):
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="unit_batch_size"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
            unit_batch_size=unit_batch_size,
        )


def test_position_sizer_base_calculate_raises_not_implemented():
    sizer = PositionSizer(GBPUSD)

    with pytest.raises(NotImplementedError):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP,
            equity=Money(1_000_000, GBPUSD.quote_currency),
            risk=Decimal("0.001"),
        )


@pytest.mark.parametrize(
    "kwargs",
    [
        pytest.param({"risk": Decimal("1e23")}, id="large risk"),
        pytest.param({"commission_rate": Decimal("1e26")}, id="large commission rate"),
        pytest.param({"exchange_rate": Decimal("1e-28")}, id="tiny exchange rate"),
    ],
)
def test_calculate_rejects_arithmetic_overflow(kwargs):
    sizer = FixedRiskSizer(GBPUSD)

    args = {
        "entry": ENTRY,
        "stop_loss": STOP,
        "equity": Money(1_000_000, GBPUSD.quote_currency),
        "risk": Decimal("0.001"),
    }
    args.update(kwargs)

    with pytest.raises(ValueError, match="overflow"):
        sizer.calculate(**args)


@pytest.mark.parametrize("cls", [PositionSizer, FixedRiskSizer])
@pytest.mark.parametrize("bad_instrument", [None, "GBP/USD", 1])
def test_constructor_rejects_invalid_instrument(cls, bad_instrument):
    with pytest.raises((TypeError, ValueError)):
        cls(bad_instrument)


@pytest.mark.parametrize(
    ("param", "value"),
    [
        pytest.param("risk", "0.001", id="risk str"),
        pytest.param("risk", 0.001, id="risk float"),
        pytest.param("risk", 1, id="risk int"),
        pytest.param("risk", None, id="risk None"),
        pytest.param("commission_rate", "0", id="commission_rate str"),
        pytest.param("commission_rate", 0, id="commission_rate int"),
        pytest.param("commission_rate", None, id="commission_rate None"),
        pytest.param("exchange_rate", 1, id="exchange_rate int"),
        pytest.param("exchange_rate", None, id="exchange_rate None"),
        pytest.param("unit_batch_size", "1", id="unit_batch_size str"),
        pytest.param("unit_batch_size", 1, id="unit_batch_size int"),
        pytest.param("unit_batch_size", None, id="unit_batch_size None"),
    ],
)
def test_calculate_rejects_non_decimal_numerics(param, value):
    sizer = FixedRiskSizer(GBPUSD)
    args = {
        "entry": ENTRY,
        "stop_loss": STOP,
        "equity": Money(1_000_000, GBPUSD.quote_currency),
        "risk": Decimal("0.001"),
    }
    args[param] = value

    with pytest.raises(TypeError, match="decimal"):
        sizer.calculate(**args)


def test_calculate_accepts_none_for_hard_limit():
    # `None` is permitted only for `hard_limit`; the other numerics reject it.
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP,
        equity=Money(1_000_000, GBPUSD.quote_currency),
        risk=Decimal("0.001"),
        hard_limit=None,
    )

    assert result >= Quantity.zero()
