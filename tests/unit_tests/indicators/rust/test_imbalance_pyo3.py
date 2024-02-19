# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import BookImbalanceRatio
from nautilus_trader.core.nautilus_pyo3 import Quantity


@pytest.fixture(scope="function")
def imbalance():
    return BookImbalanceRatio()


def test_name(imbalance: BookImbalanceRatio) -> None:
    assert imbalance.name == "BookImbalanceRatio"


def test_str_repr_returns_expected_string(imbalance: BookImbalanceRatio) -> None:
    # Arrange, Act, Assert
    assert str(imbalance) == "BookImbalanceRatio()"
    assert repr(imbalance) == "BookImbalanceRatio()"


def test_initialized_without_inputs_returns_false(imbalance: BookImbalanceRatio) -> None:
    # Arrange, Act, Assert
    assert imbalance.initialized is False


def test_initialized_with_required_inputs(imbalance: BookImbalanceRatio) -> None:
    # Arrange
    imbalance.update(Quantity.from_int(100), Quantity.from_int(100))

    # Act, Assert
    assert imbalance.initialized
    assert imbalance.has_inputs
    assert imbalance.count == 1
    assert imbalance.value == 1.0


def test_reset(imbalance: BookImbalanceRatio) -> None:
    # Arrange
    imbalance.update(Quantity.from_int(100), Quantity.from_int(100))

    # Act, Assert
    assert not imbalance.initialized
    assert not imbalance.has_inputs
    assert imbalance.count == 0
    assert imbalance.value == 0.0


def test_multiple_inputs_with_bid_imbalance(imbalance: BookImbalanceRatio) -> None:
    # Arrange
    imbalance.update(Quantity.from_int(200), Quantity.from_int(100))
    imbalance.update(Quantity.from_int(200), Quantity.from_int(100))
    imbalance.update(Quantity.from_int(200), Quantity.from_int(100))

    # Act, Assert
    assert imbalance.initialized
    assert imbalance.has_inputs
    assert imbalance.count == 3
    assert imbalance.value == 0.5


def test_multiple_inputs_with_ask_imbalance(imbalance: BookImbalanceRatio) -> None:
    # Arrange
    imbalance.update(Quantity.from_int(100), Quantity.from_int(200))
    imbalance.update(Quantity.from_int(100), Quantity.from_int(200))
    imbalance.update(Quantity.from_int(100), Quantity.from_int(200))

    # Act, Assert
    assert imbalance.initialized
    assert imbalance.has_inputs
    assert imbalance.count == 3
    assert imbalance.value == 0.5
