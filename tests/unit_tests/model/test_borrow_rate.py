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

from decimal import Decimal

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import Venue


USDT = Currency.from_str("USDT")
BYBIT = Venue("BYBIT")


class TestBorrowRatePublicExport:
    def test_importable_from_model_namespace(self):
        # Arrange, Act
        from nautilus_trader.model import BorrowRate

        # Assert: the public re-export is the same object registered on pyo3
        assert BorrowRate is nautilus_pyo3.BorrowRate

    def test_importable_from_model_data_namespace(self):
        # Arrange, Act
        from nautilus_trader.model.data import BorrowRate

        # Assert
        assert BorrowRate is nautilus_pyo3.BorrowRate

    def test_listed_in_model_dunder_all(self):
        # Arrange, Act
        import nautilus_trader.model as model

        # Assert
        assert "BorrowRate" in model.__all__


class TestBorrowRate:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        from nautilus_trader.model.data import BorrowRate

        assert (
            BorrowRate.fully_qualified_name()
            == "nautilus_trader.core.nautilus_pyo3.model:BorrowRate"
        )

    def test_new_minimal(self):
        # Arrange, Act
        from nautilus_trader.model.data import BorrowRate

        borrow_rate = BorrowRate(
            currency=USDT,
            venue=BYBIT,
            rate=Decimal("0.0001"),
            accrual_interval=60,
            ts_event=1_640_000_000_000_000_000,
            ts_init=1_640_000_000_000_000_000,
        )

        # Assert
        assert borrow_rate.currency == USDT
        assert borrow_rate.venue == BYBIT
        assert borrow_rate.rate == Decimal("0.0001")
        assert borrow_rate.accrual_interval == 60
        assert borrow_rate.next_accrual_ns is None
        assert borrow_rate.borrow_limit is None
        assert borrow_rate.ts_event == 1_640_000_000_000_000_000
        assert borrow_rate.ts_init == 1_640_000_000_000_000_000

    def test_new_complete(self):
        # Arrange, Act
        from nautilus_trader.model.data import BorrowRate

        borrow_rate = BorrowRate(
            currency=USDT,
            venue=BYBIT,
            rate=Decimal("0.0001"),
            accrual_interval=60,
            ts_event=1_640_000_000_000_000_000,
            ts_init=1_640_000_000_000_000_000,
            next_accrual_ns=1_640_000_100_000_000_000,
            borrow_limit=Money(1_000_000.00, USDT),
        )

        # Assert
        assert borrow_rate.next_accrual_ns == 1_640_000_100_000_000_000
        assert borrow_rate.borrow_limit == Money(1_000_000.00, USDT)

    def test_dict_round_trip(self):
        # Arrange
        from nautilus_trader.model.data import BorrowRate

        borrow_rate = BorrowRate(
            currency=USDT,
            venue=BYBIT,
            rate=Decimal("0.0001"),
            accrual_interval=60,
            ts_event=1_640_000_000_000_000_000,
            ts_init=1_640_000_000_000_000_000,
        )

        # Act
        result = BorrowRate.from_dict(borrow_rate.to_dict())

        # Assert
        assert result == borrow_rate
