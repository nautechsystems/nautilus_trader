# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.execution.messages import ExecAlgorithmSpecification
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId


class TestExecAlgorithmSpecification:
    def test_exec_algorithm_spec_properties(self):
        # Arrange, Act
        exec_algorithm_spec = ExecAlgorithmSpecification(
            client_order_id=ClientOrderId("O-123456789"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        # Assert
        assert exec_algorithm_spec.exec_algorithm_id.value == "VWAP"

    def test_exec_algorithm_spec_equality(self):
        # Arrange
        exec_algorithm_spec1 = ExecAlgorithmSpecification(
            client_order_id=ClientOrderId("O-123456789"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        exec_algorithm_spec2 = ExecAlgorithmSpecification(
            client_order_id=ClientOrderId("O-123456789"),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        # Act, Assert
        assert exec_algorithm_spec1 == exec_algorithm_spec1
        assert exec_algorithm_spec1 != exec_algorithm_spec2

    def test_exec_algorithm_spec_hash_str_repr(self):
        # Arrange, Act
        exec_algorithm_spec = ExecAlgorithmSpecification(
            client_order_id=ClientOrderId("O-123456789"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        # Assert
        assert isinstance(hash(exec_algorithm_spec), int)
        assert (
            str(exec_algorithm_spec)
            == "ExecAlgorithmSpecification(client_order_id=O-123456789, exec_algorithm_id=VWAP, params={'max_percentage': 100.0, 'start': 0, 'end': 1})"  # noqa
        )
        assert (
            repr(exec_algorithm_spec)
            == "ExecAlgorithmSpecification(client_order_id=O-123456789, exec_algorithm_id=VWAP, params={'max_percentage': 100.0, 'start': 0, 'end': 1})"  # noqa
        )
