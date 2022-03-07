# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.ftx.core.types import FTXTicker
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs.identities import TestIdStubs


class TestFTXDataTypes:
    def test_ftx_ticker_repr(self):
        # Arrange
        ticker = FTXTicker(
            instrument_id=TestIdStubs.ethusd_ftx_id(),
            bid=Price.from_str("3717.4"),
            ask=Price.from_str("3717.5"),
            bid_size=Quantity.from_str("23.052"),
            ask_size=Quantity.from_str("5.654"),
            last=Price.from_str("3717.4"),
            ts_event=1641012223016193,
            ts_init=1641012223092101,
        )

        # Act, Assert
        assert (
            repr(ticker)
            == "FTXTicker(instrument_id=ETH-PERP.FTX, bid=3717.4, ask=3717.5, bid_size=23.052, ask_size=5.654, last=3717.4, ts_event=1641012223016193, ts_init=1641012223092101)"  # noqa
        )

    def test_ftx_ticker_to_and_from_dict(self):
        # Arrange
        ticker = FTXTicker(
            instrument_id=TestIdStubs.ethusd_ftx_id(),
            bid=Price.from_str("3717.4"),
            ask=Price.from_str("3717.5"),
            bid_size=Quantity.from_str("23.052"),
            ask_size=Quantity.from_str("5.654"),
            last=Price.from_str("3717.4"),
            ts_event=1641012223016193,
            ts_init=1641012223092101,
        )

        # Act
        values = ticker.to_dict(ticker)

        # Assert
        FTXTicker.from_dict(values)
        assert values == {
            "type": "FTXTicker",
            "instrument_id": "ETH-PERP.FTX",
            "bid": "3717.4",
            "ask": "3717.5",
            "bid_size": "23.052",
            "ask_size": "5.654",
            "last": "3717.4",
            "ts_event": 1641012223016193,
            "ts_init": 1641012223092101,
        }
