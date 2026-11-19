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
Test the FinFeed Python module registration.
"""

from importlib import import_module

import pytest

from nautilus_trader import _libnautilus
from nautilus_trader.adapters.finfeed import finfeed_instrument_id
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId


def test_module_registration() -> None:
    """
    Test the extension registers FinFeed for dotted imports.
    """
    module = import_module("nautilus_trader._libnautilus.finfeed")
    client_id = module.FINFEED_CLIENT_ID

    assert module is _libnautilus.finfeed
    assert module.__name__ == "finfeed"
    assert module.FINFEED == "FINFEED"
    assert client_id == ClientId("FINFEED")


@pytest.mark.parametrize(
    ("exchange_id", "market_id", "expected"),
    [
        ("POLYMARKET", "TEST-OUTCOME_YES", "TEST-OUTCOME_YES.FINFEED_POLYMARKET"),
        ("KALSHI", "TEST-KALSHI-30_NO", "TEST-KALSHI-30_NO.FINFEED_KALSHI"),
    ],
)
def test_finfeed_instrument_id_uses_the_exchange_venue(
    exchange_id: str,
    market_id: str,
    expected: str,
) -> None:
    """
    Build an instrument ID from an exchange and an exact outcome ID.
    """
    instrument_id = finfeed_instrument_id(exchange_id, market_id)

    assert instrument_id == InstrumentId.from_str(expected)


@pytest.mark.parametrize(
    ("exchange_id", "market_id", "match"),
    [
        ("UNKNOWN", "TEST-OUTCOME_YES", "unknown exchange UNKNOWN"),
        ("POLYMARKET", "..", "invalid outcome ID"),
    ],
)
def test_finfeed_instrument_id_rejects_invalid_input(
    exchange_id: str,
    market_id: str,
    match: str,
) -> None:
    """
    Reject an unknown exchange or an invalid outcome ID.
    """
    with pytest.raises(ValueError, match=match):
        finfeed_instrument_id(exchange_id, market_id)
