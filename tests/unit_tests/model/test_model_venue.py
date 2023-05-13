# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data.venue import InstrumentClose
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.data.venue import VenueStatusUpdate
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVenue:
    def test_venue_status(self):
        # Arrange
        update = VenueStatusUpdate(
            venue=Venue("BINANCE"),
            status=MarketStatus.OPEN,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert VenueStatusUpdate.from_dict(VenueStatusUpdate.to_dict(update)) == update
        assert repr(update) == "VenueStatusUpdate(venue=BINANCE, status=OPEN)"

    def test_instrument_status(self):
        # Arrange
        update = InstrumentStatusUpdate(
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            status=MarketStatus.PAUSE,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert InstrumentStatusUpdate.from_dict(InstrumentStatusUpdate.to_dict(update)) == update
        assert repr(update) == "InstrumentStatusUpdate(instrument_id=BTCUSDT.BINANCE, status=PAUSE)"

    def test_instrument_close(self):
        # Arrange
        update = InstrumentClose(
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            close_price=Price(100.0, precision=0),
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert InstrumentClose.from_dict(InstrumentClose.to_dict(update)) == update
        assert (
            "InstrumentClose(instrument_id=BTCUSDT.BINANCE, close_price=100, close_type=CONTRACT_EXPIRED)"
            == repr(update)
        )
