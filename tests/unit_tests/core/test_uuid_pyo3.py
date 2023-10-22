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

import pickle

from nautilus_trader.core.nautilus_pyo3 import UUID4


class TestUUID:
    def test_pickling_round_trip(self):
        # Arrange
        uuid = UUID4()

        # Act
        pickled = pickle.dumps(uuid)
        unpickled = pickle.loads(pickled)  # noqa

        # Assert
        assert unpickled == uuid

    def test_equality(self):
        # Arrange, Act
        uuid1 = UUID4("c2988650-5beb-8af8-e714-377a3a1c26ed")
        uuid2 = UUID4("c2988650-5beb-8af8-e714-377a3a1c26ed")
        uuid3 = UUID4("a2988650-5beb-8af8-e714-377a3a1c26ed")

        # Assert
        assert uuid1 == uuid1
        assert uuid1 == uuid2
        assert uuid2 != uuid3

    def test_hash(self):
        # Arrange
        uuid1 = UUID4("c2988650-5beb-8af8-e714-377a3a1c26ed")
        uuid2 = UUID4("c2988650-5beb-8af8-e714-377a3a1c26ed")

        # Act, Assert
        assert isinstance((hash(uuid1)), int)
        assert hash(uuid1) == hash(uuid2)

    def test_str_and_repr(self):
        # Arrange
        uuid = UUID4("c2988650-5beb-8af8-e714-377a3a1c26ed")

        # Act, Assert
        assert uuid.value == "c2988650-5beb-8af8-e714-377a3a1c26ed"
        assert str(uuid) == "c2988650-5beb-8af8-e714-377a3a1c26ed"
        assert repr(uuid) == "UUID4('c2988650-5beb-8af8-e714-377a3a1c26ed')"

    def test_uuid4_produces_valid_uuid4(self):
        # Arrange, Act
        result = UUID4()

        # Assert
        assert isinstance(result, UUID4)
        assert len(str(result)) == 36
        assert len(str(result).replace("-", "")) == 32
