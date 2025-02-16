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

import datetime

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.accounting.calculators import RolloverInterestCalculator
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestIdStubs.audusd_id()
GBPUSD_SIM = TestIdStubs.gbpusd_id()
USDJPY_SIM = TestIdStubs.usdjpy_id()


class TestExchangeRateCalculator:
    def test_get_rate_when_from_currency_equals_to_currency_returns_one(self):
        # Arrange
        bid_rates = {"AUD/USD": 0.80000}
        ask_rates = {"AUD/USD": 0.80010}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            USD.code,
            USD.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 1

    def test_get_rate_when_no_currency_rate_returns_zero(self):
        # Arrange
        bid_rates = {"AUD/USD": 0.80000}
        ask_rates = {"AUD/USD": 0.80010}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            USD.code,
            JPY.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result is None

    def test_get_rate(self):
        # Arrange
        bid_rates = {"AUD/USD": 0.80000}
        ask_rates = {"AUD/USD": 0.80010}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            AUD.code,
            USD.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 0.80000

    def test_get_rate_when_symbol_has_slash(self):
        # Arrange
        bid_rates = {"AUD/USD": 0.80000}
        ask_rates = {"AUD/USD": 0.80010}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            AUD.code,
            USD.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 0.80000

    def test_get_rate_for_inverse1(self):
        # Arrange
        bid_rates = {"BTC/USD": 10501.5}
        ask_rates = {"BTC/USD": 10500.0}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            USD.code,
            BTC.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 9.522449173927534e-05

    def test_get_rate_for_inverse2(self):
        # Arrange
        bid_rates = {"USD/JPY": 110.100}
        ask_rates = {"USD/JPY": 110.130}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            JPY.code,
            USD.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 0.009082652134423252

    def test_calculate_exchange_rate_by_inference(self):
        # Arrange
        bid_rates = {
            "USD/JPY": 110.100,
            "AUD/USD": 0.80000,
        }
        ask_rates = {
            "USD/JPY": 110.130,
            "AUD/USD": 0.80010,
        }

        # Act
        result1 = nautilus_pyo3.get_exchange_rate(
            JPY.code,
            AUD.code,
            nautilus_pyo3.PriceType.BID,
            bid_rates,
            ask_rates,
        )

        result2 = nautilus_pyo3.get_exchange_rate(
            AUD.code,
            JPY.code,
            nautilus_pyo3.PriceType.ASK,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result1 == 0.011353315168029066
        assert result2 == 88.115013

    def test_calculate_exchange_rate_for_mid_price_type(self):
        # Arrange
        bid_rates = {"USD/JPY": 110.100}
        ask_rates = {"USD/JPY": 110.130}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            JPY.code,
            USD.code,
            nautilus_pyo3.PriceType.MID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 0.009081414884438995

    def test_calculate_exchange_rate_for_mid_price_type2(self):
        # Arrange
        bid_rates = {"USD/JPY": 110.100}
        ask_rates = {"USD/JPY": 110.130}

        # Act
        result = nautilus_pyo3.get_exchange_rate(
            USD.code,
            JPY.code,
            nautilus_pyo3.PriceType.MID,
            bid_rates,
            ask_rates,
        )

        # Assert
        assert result == 110.115


class TestRolloverInterestCalculator:
    def setup(self):
        # Fixture Setup
        self.data = pd.read_csv(TEST_DATA_DIR / "short-term-interest.csv")

    def test_rate_dataframe_returns_correct_dataframe(self):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate_data = calculator.get_rate_data()

        # Assert
        assert isinstance(rate_data, dict)

    def test_calc_overnight_fx_rate_with_audusd_on_unix_epoch_returns_correct_rate(
        self,
    ):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_SIM, UNIX_EPOCH)

        # Assert
        assert rate == -8.52054794520548e-05

    def test_calc_overnight_fx_rate_with_audusd_on_later_date_returns_correct_rate(
        self,
    ):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_SIM, datetime.date(2018, 2, 1))

        # Assert
        assert rate == -2.739726027397263e-07

    def test_calc_overnight_fx_rate_with_audusd_on_impossible_dates_returns_zero(self):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act, Assert
        with pytest.raises(RuntimeError):
            calculator.calc_overnight_rate(AUDUSD_SIM, datetime.date(1900, 1, 1))

        with pytest.raises(RuntimeError):
            calculator.calc_overnight_rate(AUDUSD_SIM, datetime.date(3000, 1, 1))
