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

import pickle
from decimal import Decimal

from nautilus_trader.model import FundingRateUpdate


def test_funding_rate_update_construction(audusd_id):
    funding = FundingRateUpdate(
        instrument_id=audusd_id,
        rate=Decimal("0.0001"),
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
        interval=480,
        next_funding_ns=2_000_000_000,
    )

    assert funding.instrument_id == audusd_id
    assert funding.rate == Decimal("0.0001")
    assert funding.interval == 480
    assert funding.next_funding_ns == 2_000_000_000
    assert funding.ts_event == 1_000_000_000
    assert funding.ts_init == 1_000_000_001


def test_funding_rate_update_equality(audusd_id):
    f1 = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)
    f2 = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)

    assert f1 == f2


def test_funding_rate_update_hash(audusd_id):
    f1 = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)
    f2 = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)

    assert hash(f1) == hash(f2)


def test_funding_rate_update_repr(audusd_id):
    funding = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0)
    r = repr(funding)

    assert "FundingRateUpdate" in r
    assert "AUD/USD.SIM" in r
    assert "0.0001" in r


def test_funding_rate_update_to_dict_and_from_dict_roundtrip(audusd_id):
    funding = FundingRateUpdate(
        instrument_id=audusd_id,
        rate=Decimal("0.0001"),
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
        interval=480,
        next_funding_ns=2_000_000_000,
    )

    d = FundingRateUpdate.to_dict(funding)
    restored = FundingRateUpdate.from_dict(d)

    assert d["type"] == "FundingRateUpdate"
    assert restored == funding


def test_funding_rate_update_fully_qualified_name():
    assert "FundingRateUpdate" in FundingRateUpdate.fully_qualified_name()


def test_funding_rate_update_pickle_roundtrip(audusd_id):
    funding = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)

    pickled = pickle.dumps(funding)
    unpickled = pickle.loads(pickled)  # noqa: S301

    assert unpickled == funding


def test_funding_rate_update_json_roundtrip(audusd_id):
    funding = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)

    json_bytes = funding.to_json()
    restored = FundingRateUpdate.from_json(json_bytes)

    assert restored == funding


def test_funding_rate_update_msgpack_roundtrip(audusd_id):
    funding = FundingRateUpdate(audusd_id, Decimal("0.0001"), 0, 0, interval=480)

    msgpack_bytes = funding.to_msgpack()
    restored = FundingRateUpdate.from_msgpack(msgpack_bytes)

    assert restored == funding


def test_funding_rate_update_get_metadata(audusd_id):
    metadata = FundingRateUpdate.get_metadata(audusd_id)

    assert metadata["instrument_id"] == "AUD/USD.SIM"


def test_funding_rate_update_get_fields():
    fields = FundingRateUpdate.get_fields()

    assert "rate" in fields
    assert "ts_event" in fields
    assert "ts_init" in fields
