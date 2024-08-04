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

from nautilus_trader.core.data import Data
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.identifiers import InstrumentId


@customdataclass
class GreeksTestData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    delta: float = 0.0

    def __repr__(self):
        return f"{self(type).__name__}(instrument_id={self.instrument_id}, delta={self.delta:.2f}, ts_event={self.ts_event}, ts_init={self._ts_init})"


def test_customdata_decorator_properties() -> None:
    # Arrange, Act
    data = GreeksTestData(ts_event=2, ts_init=1)

    # Assert
    assert data.ts_event == 2
    assert data.ts_init == 1


def test_customdata_decorator_dict() -> None:
    # Arrange
    data = GreeksTestData(ts_event=2, ts_init=1)

    # Act
    data_dict = data.to_dict()

    # Assert
    assert data_dict == {
        "instrument_id": "ES.GLBX",
        "delta": 0.0,
        "ts_event": 2,
        "ts_init": 1,
    }
    # assert GreeksTestData.from_dict(data_dict) == data
