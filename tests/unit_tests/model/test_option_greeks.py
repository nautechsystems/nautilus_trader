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

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import OptionGreeks
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


def _make_greeks(convention=None) -> OptionGreeks:
    kwargs: dict = {
        "instrument_id": BTCUSDT_BINANCE.id,
        "delta": 0.55,
        "gamma": 0.02,
        "vega": 0.15,
        "theta": -0.05,
        "rho": 0.01,
        "mark_iv": 0.25,
        "bid_iv": 0.24,
        "ask_iv": 0.26,
        "underlying_price": 155.0,
        "open_interest": 1000.0,
        "ts_event": 1,
        "ts_init": 2,
    }

    if convention is not None:
        kwargs["convention"] = convention
    return OptionGreeks(**kwargs)


class TestOptionGreeks:
    def test_default_convention_is_black_scholes(self):
        # Arrange, Act
        greeks = _make_greeks()

        # Assert
        assert greeks.convention == nautilus_pyo3.GreeksConvention.BLACK_SCHOLES

    @pytest.mark.parametrize(
        "convention",
        [
            nautilus_pyo3.GreeksConvention.BLACK_SCHOLES,
            nautilus_pyo3.GreeksConvention.PRICE_ADJUSTED,
        ],
    )
    def test_explicit_convention_is_preserved(self, convention):
        # Arrange, Act
        greeks = _make_greeks(convention=convention)

        # Assert
        assert greeks.convention == convention

    def test_convention_is_readonly(self):
        # Arrange
        greeks = _make_greeks()

        # Act, Assert
        with pytest.raises(AttributeError):
            greeks.convention = nautilus_pyo3.GreeksConvention.PRICE_ADJUSTED

    @pytest.mark.parametrize(
        "convention",
        [
            nautilus_pyo3.GreeksConvention.BLACK_SCHOLES,
            nautilus_pyo3.GreeksConvention.PRICE_ADJUSTED,
        ],
    )
    def test_from_pyo3_preserves_convention(self, convention):
        # Arrange
        pyo3_greeks = nautilus_pyo3.OptionGreeks(
            nautilus_pyo3.InstrumentId.from_str(str(BTCUSDT_BINANCE.id)),
            0.55,
            0.02,
            0.15,
            -0.05,
            0.01,
            0.25,
            0.24,
            0.26,
            155.0,
            1000.0,
            1,
            2,
            convention,
        )

        # Act
        greeks = OptionGreeks.from_pyo3(pyo3_greeks)

        # Assert
        assert greeks.convention == convention

    @pytest.mark.parametrize(
        "convention",
        [
            nautilus_pyo3.GreeksConvention.BLACK_SCHOLES,
            nautilus_pyo3.GreeksConvention.PRICE_ADJUSTED,
        ],
    )
    def test_pyo3_roundtrip_preserves_convention(self, convention):
        # Arrange
        original = _make_greeks(convention=convention)

        # Act
        roundtripped = OptionGreeks.from_pyo3(original.to_pyo3())

        # Assert
        assert roundtripped.convention == convention
        assert roundtripped.delta == original.delta
        assert roundtripped.gamma == original.gamma
        assert roundtripped.vega == original.vega
        assert roundtripped.theta == original.theta
        assert roundtripped.rho == original.rho
