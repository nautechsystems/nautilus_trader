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
"""
Test index price behavior.
"""

import pickle
import re

import pytest

from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price


BTCUSDT_BINANCE = InstrumentId.from_str("BTCUSDT.BINANCE")


def test_fully_qualified_name() -> None:
    """
    Test fully qualified name.
    """
    assert IndexPriceUpdate.fully_qualified_name() == "nautilus_trader.model:IndexPriceUpdate"


def test_hash_str_and_repr() -> None:
    """
    Test hash str and repr.
    """
    update = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    assert isinstance(hash(update), int)
    assert str(update) == "BTCUSDT.BINANCE,100000.00,1,2"
    assert repr(update) == "IndexPriceUpdate(BTCUSDT.BINANCE,100000.00,1,2)"


def test_to_dict() -> None:
    """
    Test to dict.
    """
    update = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    result = IndexPriceUpdate.to_dict(update)

    assert result == {
        "type": "IndexPriceUpdate",
        "instrument_id": "BTCUSDT.BINANCE",
        "value": "100000.00",
        "ts_event": 1,
        "ts_init": 2,
    }


def test_from_dict_roundtrip() -> None:
    """
    Test from dict roundtrip.
    """
    update = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    result = IndexPriceUpdate.from_dict(IndexPriceUpdate.to_dict(update))

    assert result == update


def test_equality() -> None:
    """
    Test equality.
    """
    update1 = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )
    update2 = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )

    assert update1 == update2


def test_pickle_roundtrip() -> None:
    """
    Test pickle roundtrip.
    """
    update = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.000000000000000001"),
        ts_event=1,
        ts_init=2,
    )

    restored = pickle.loads(pickle.dumps(update))

    assert restored == update
    assert restored.value.precision == 18
    assert restored.ts_event == 1
    assert restored.ts_init == 2


@pytest.mark.parametrize(
    ("index", "value", "message"),
    [
        (
            1,
            -170_141_183_460_460_000_000_000_000_001,
            "raw value -170141183460460000000000000001 outside valid range "
            "[-170141183460460000000000000000, 170141183460460000000000000000]",
        ),
        (2, 255, "`precision` exceeded maximum `WEI_PRECISION` (18), was 255"),
    ],
)
def test_setstate_rejects_invalid_state_without_mutation(
    index: int,
    value: object,
    message: str,
) -> None:
    """
    Test setstate rejects invalid state without mutation.
    """
    update = IndexPriceUpdate(
        instrument_id=BTCUSDT_BINANCE,
        value=Price.from_str("100000.00"),
        ts_event=1,
        ts_init=2,
    )
    other = IndexPriceUpdate(
        instrument_id=InstrumentId.from_str("ETHUSDT.BINANCE"),
        value=Price.from_str("3500.000"),
        ts_event=5,
        ts_init=6,
    )
    original_state = update.__getstate__()
    state = list(other.__getstate__())
    state[index] = value

    with pytest.raises(ValueError, match=re.escape(message)) as exc_info:
        update.__setstate__(tuple(state))

    assert str(exc_info.value) == message
    assert update.__getstate__() == original_state
