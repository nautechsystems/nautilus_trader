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

from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import Price


def test_instrument_status_construction(audusd_id):
    status = InstrumentStatus(
        instrument_id=audusd_id,
        action=MarketStatusAction.TRADING,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
        reason="Session open",
        trading_event="open",
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )

    assert status.instrument_id == audusd_id
    assert status.action == MarketStatusAction.TRADING
    assert status.ts_event == 1_000_000_000
    assert status.ts_init == 1_000_000_001
    assert status.reason == "Session open"
    assert status.trading_event == "open"
    assert status.is_trading is True
    assert status.is_quoting is True
    assert status.is_short_sell_restricted is False


def test_instrument_status_equality(audusd_id):
    s1 = InstrumentStatus(audusd_id, MarketStatusAction.TRADING, 0, 0)
    s2 = InstrumentStatus(audusd_id, MarketStatusAction.TRADING, 0, 0)

    assert s1 == s2


def test_instrument_status_repr(audusd_id):
    status = InstrumentStatus(audusd_id, MarketStatusAction.TRADING, 0, 0)
    r = repr(status)

    assert "InstrumentStatus" in r
    assert "AUD/USD.SIM" in r
    assert "TRADING" in r


def test_instrument_status_to_dict_and_from_dict_roundtrip(audusd_id):
    status = InstrumentStatus(
        instrument_id=audusd_id,
        action=MarketStatusAction.TRADING,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
        reason="Session open",
        trading_event="open",
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )

    d = InstrumentStatus.to_dict(status)
    restored = InstrumentStatus.from_dict(d)

    assert d["type"] == "InstrumentStatus"
    assert restored == status


def test_instrument_status_fully_qualified_name():
    assert "InstrumentStatus" in InstrumentStatus.fully_qualified_name()


def test_instrument_status_json_roundtrip(audusd_id):
    status = InstrumentStatus(audusd_id, MarketStatusAction.TRADING, 0, 0)

    json_bytes = status.to_json_bytes()
    restored = InstrumentStatus.from_json(json_bytes)

    assert restored == status


def test_instrument_status_msgpack_roundtrip(audusd_id):
    status = InstrumentStatus(audusd_id, MarketStatusAction.TRADING, 0, 0)

    msgpack_bytes = status.to_msgpack_bytes()
    restored = InstrumentStatus.from_msgpack(msgpack_bytes)

    assert restored == status


def test_instrument_status_get_metadata(audusd_id):
    metadata = InstrumentStatus.get_metadata(audusd_id)

    assert metadata["instrument_id"] == "AUD/USD.SIM"


def test_instrument_close_construction(audusd_id):
    close = InstrumentClose(
        instrument_id=audusd_id,
        close_price=Price.from_str("0.75000"),
        close_type=InstrumentCloseType.END_OF_SESSION,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
    )

    assert close.instrument_id == audusd_id
    assert close.close_price == Price.from_str("0.75000")
    assert close.close_type == InstrumentCloseType.END_OF_SESSION
    assert close.ts_event == 1_000_000_000
    assert close.ts_init == 1_000_000_001


def test_instrument_close_equality(audusd_id):
    c1 = InstrumentClose(
        audusd_id,
        Price.from_str("0.75000"),
        InstrumentCloseType.END_OF_SESSION,
        0,
        0,
    )
    c2 = InstrumentClose(
        audusd_id,
        Price.from_str("0.75000"),
        InstrumentCloseType.END_OF_SESSION,
        0,
        0,
    )

    assert c1 == c2


def test_instrument_close_repr(audusd_id):
    close = InstrumentClose(
        audusd_id,
        Price.from_str("0.75000"),
        InstrumentCloseType.END_OF_SESSION,
        0,
        0,
    )
    r = repr(close)

    assert "0.75000" in r
    assert "END_OF_SESSION" in r


def test_instrument_close_to_dict_and_from_dict_roundtrip(audusd_id):
    close = InstrumentClose(
        instrument_id=audusd_id,
        close_price=Price.from_str("0.75000"),
        close_type=InstrumentCloseType.END_OF_SESSION,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
    )

    d = InstrumentClose.to_dict(close)
    restored = InstrumentClose.from_dict(d)

    assert d["type"] == "InstrumentClose"
    assert restored == close


def test_instrument_close_fully_qualified_name():
    assert "InstrumentClose" in InstrumentClose.fully_qualified_name()


def test_instrument_close_json_roundtrip(audusd_id):
    close = InstrumentClose(
        audusd_id,
        Price.from_str("0.75000"),
        InstrumentCloseType.END_OF_SESSION,
        0,
        0,
    )

    json_bytes = close.to_json_bytes()
    restored = InstrumentClose.from_json(json_bytes)

    assert restored == close


def test_instrument_close_msgpack_roundtrip(audusd_id):
    close = InstrumentClose(
        audusd_id,
        Price.from_str("0.75000"),
        InstrumentCloseType.END_OF_SESSION,
        0,
        0,
    )

    msgpack_bytes = close.to_msgpack_bytes()
    restored = InstrumentClose.from_msgpack(msgpack_bytes)

    assert restored == close


def test_instrument_close_get_metadata(audusd_id):
    metadata = InstrumentClose.get_metadata(audusd_id, 5)

    assert metadata["instrument_id"] == "AUD/USD.SIM"


def test_instrument_close_get_fields():
    fields = InstrumentClose.get_fields()

    assert "close_price" in fields
    assert "ts_event" in fields
    assert "ts_init" in fields
