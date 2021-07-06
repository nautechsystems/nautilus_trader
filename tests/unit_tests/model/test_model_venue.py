# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import VenueStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.venue import InstrumentClosePrice
from nautilus_trader.model.venue import InstrumentStatusUpdate
from nautilus_trader.model.venue import VenueStatusUpdate
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVenue:
    def test_venue_status(self):
        # Arrange
        update = VenueStatusUpdate(
            venue=Venue("BINANCE"),
            status=VenueStatus.OPEN,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert VenueStatusUpdate.from_dict(VenueStatusUpdate.to_dict(update)) == update
        assert "VenueStatusUpdate(venue=BINANCE, status=OPEN)" == repr(update)

    def test_instrument_status(self):
        # Arrange
        update = InstrumentStatusUpdate(
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            status=InstrumentStatus.PAUSE,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert InstrumentStatusUpdate.from_dict(InstrumentStatusUpdate.to_dict(update)) == update
        assert "InstrumentStatusUpdate(instrument_id=BTC/USDT.BINANCE, status=PAUSE)" == repr(
            update
        )

    def test_instrument_close_price(self):
        # Arrange
        update = InstrumentClosePrice(
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            close_price=Price(100.0, precision=0),
            close_type=InstrumentCloseType.EXPIRED,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert InstrumentClosePrice.from_dict(InstrumentClosePrice.to_dict(update)) == update
        assert (
            "InstrumentClosePrice(instrument_id=BTC/USDT.BINANCE, close_price=100, close_type=EXPIRED)"
            == repr(update)
        )
