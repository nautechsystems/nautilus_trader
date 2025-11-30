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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_BINARY_OPTION = TestInstrumentProviderPyo3.binary_option()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.binary_option()
    item_2 = TestInstrumentProviderPyo3.binary_option()
    assert item_1 == item_2


def test_hash():
    assert hash(_BINARY_OPTION) == hash(_BINARY_OPTION)


def test_to_dict():
    result = _BINARY_OPTION.to_dict()
    assert nautilus_pyo3.BinaryOption.from_dict(result) == _BINARY_OPTION
    assert result == {
        "type": "BinaryOption",
        "id": "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905.POLYMARKET",
        "raw_symbol": "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
        "asset_class": "ALTERNATIVE",
        "currency": "USDC",
        "activation_ns": 0,
        "expiration_ns": 1704067200000000000,
        "price_precision": 3,
        "size_precision": 2,
        "price_increment": "0.001",
        "size_increment": "0.01",
        "margin_init": "0",
        "margin_maint": "0",
        "info": {},
        "maker_fee": "0",
        "taker_fee": "0",
        "ts_event": 0,
        "ts_init": 0,
        "outcome": "Yes",
        "description": "Will the outcome of this market be 'Yes'?",
        "max_quantity": None,
        "min_quantity": "5",
        "max_notional": None,
        "min_notional": None,
        "max_price": None,
        "min_price": None,
    }


# TODO: Not implemented
# def test_pyo3_cython_conversion():
#     binary_option_pyo3 = TestInstrumentProviderPyo3.binary_option()
#     binary_option_pyo3_dict = binary_option_pyo3.to_dict()
#     binary_option_cython = BinaryOption.from_pyo3(binary_option_pyo3)
#     binary_option_cython_dict = BinaryOption.to_dict(binary_option_cython)
#     binary_option_pyo3_back = nautilus_pyo3.BinaryOption.from_dict(binary_option_cython_dict)
#     assert binary_option_pyo3 == binary_option_pyo3_back
#     assert binary_option_pyo3_dict == binary_option_cython_dict
