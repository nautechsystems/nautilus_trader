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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.model.instruments import IndexInstrument
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_INDEX = TestInstrumentProviderPyo3.index_instrument()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.index_instrument()
    item_2 = TestInstrumentProviderPyo3.index_instrument()
    assert item_1 == item_2


def test_hash():
    assert hash(_INDEX) == hash(_INDEX)


def test_properties():
    assert _INDEX.id == InstrumentId.from_str("SPX.INDEX")


def test_to_dict():
    result = _INDEX.to_dict()
    assert nautilus_pyo3.IndexInstrument.from_dict(result) == _INDEX
    assert result == {
        "type": "IndexInstrument",
        "id": "SPX.INDEX",
        "raw_symbol": "SPX",
        "currency": "USD",
        "price_precision": 2,
        "size_precision": 0,
        "price_increment": "0.01",
        "size_increment": "1",
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
    }


def test_pyo3_cython_conversion():
    index_pyo3 = TestInstrumentProviderPyo3.index_instrument()
    index_pyo3_dict = index_pyo3.to_dict()
    index_cython = IndexInstrument.from_pyo3(index_pyo3)
    index_cython_dict = IndexInstrument.to_dict(index_cython)
    del index_cython_dict["tick_scheme_name"]
    index_pyo3_back = nautilus_pyo3.IndexInstrument.from_dict(index_cython_dict)
    assert index_cython_dict == index_pyo3_dict
    assert index_pyo3 == index_pyo3_back
