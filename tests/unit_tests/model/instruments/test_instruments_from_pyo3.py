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

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Cfd
from nautilus_trader.model.instruments import Commodity
from nautilus_trader.model.instruments import IndexInstrument
from nautilus_trader.model.instruments import instruments_from_pyo3
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


@pytest.mark.parametrize(
    ("pyo3_instrument", "expected_type"),
    [
        (TestInstrumentProviderPyo3.cfd(), Cfd),
        (TestInstrumentProviderPyo3.commodity(), Commodity),
        (TestInstrumentProviderPyo3.index_instrument(), IndexInstrument),
    ],
)
def test_instruments_from_pyo3_converts_supported_instrument(
    pyo3_instrument,
    expected_type,
):
    [cython_instrument] = instruments_from_pyo3([pyo3_instrument])

    assert isinstance(cython_instrument, expected_type)
    assert cython_instrument.id == InstrumentId.from_str(pyo3_instrument.id.value)
