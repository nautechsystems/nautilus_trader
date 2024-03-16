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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import OptionsSpread
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_OPTIONS_SPREAD = TestInstrumentProviderPyo3.options_spread()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.options_spread()
    item_2 = TestInstrumentProviderPyo3.options_spread()
    assert item_1 == item_2


def test_hash():
    assert hash(_OPTIONS_SPREAD) == hash(_OPTIONS_SPREAD)


def test_to_dict():
    result = _OPTIONS_SPREAD.to_dict()
    assert nautilus_pyo3.OptionsSpread.from_dict(result) == _OPTIONS_SPREAD
    assert result == {
        "type": "OptionsSpread",
        "id": "UD:U$: GN 2534559.GLBX",
        "raw_symbol": "UD:U$: GN 2534559",
        "asset_class": "FX",
        "exchange": "XCME",
        "underlying": "SR3",
        "strategy_type": "GN",
        "activation_ns": 1699304047000000000,
        "expiration_ns": 1708729140000000000,
        "currency": "USDT",
        "price_precision": 2,
        "price_increment": "0.01",
        "size_increment": "1",
        "size_precision": 0,
        "multiplier": "1",
        "lot_size": "1",
        "max_quantity": None,
        "min_quantity": "1",
        "max_price": None,
        "min_price": None,
        "margin_init": "0",
        "margin_maint": "0",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_legacy_options_contract_from_pyo3():
    option = OptionsSpread.from_pyo3(_OPTIONS_SPREAD)

    assert option.id.value == "UD:U$: GN 2534559.GLBX"


def test_pyo3_cython_conversion():
    options_spread_pyo3 = TestInstrumentProviderPyo3.options_spread()
    options_spread_pyo3_dict = options_spread_pyo3.to_dict()
    options_spread_cython = OptionsSpread.from_pyo3(options_spread_pyo3)
    options_spread_cython_dict = OptionsSpread.to_dict(options_spread_cython)
    options_spread_pyo3_back = nautilus_pyo3.OptionsSpread.from_dict(options_spread_cython_dict)
    assert options_spread_cython_dict == options_spread_pyo3_dict
    assert options_spread_pyo3 == options_spread_pyo3_back
