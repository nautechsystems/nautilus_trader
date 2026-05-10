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

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Price


BTCUSDT_BINANCE = InstrumentId.from_str("BTCUSDT.BINANCE")


def test_fully_qualified_name():
    assert "MarkPriceUpdate" in MarkPriceUpdate.fully_qualified_name()


def test_hash_str_and_repr():
    update = MarkPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    assert isinstance(hash(update), int)
    assert str(update) == "BTCUSDT.BINANCE,100000.00,1,2"
    assert repr(update) == "MarkPriceUpdate(BTCUSDT.BINANCE,100000.00,1,2)"


def test_to_dict():
    update = MarkPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    result = MarkPriceUpdate.to_dict(update)

    assert result == {
        "type": "MarkPriceUpdate",
        "instrument_id": "BTCUSDT.BINANCE",
        "value": "100000.00",
        "ts_event": 1,
        "ts_init": 2,
    }


def test_from_dict_roundtrip():
    update = MarkPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    result = MarkPriceUpdate.from_dict(MarkPriceUpdate.to_dict(update))

    assert result == update


def test_equality():
    update1 = MarkPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )
    update2 = MarkPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    assert update1 == update2
