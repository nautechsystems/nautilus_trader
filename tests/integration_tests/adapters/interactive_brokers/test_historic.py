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

from nautilus_trader.adapters.interactive_brokers.historic import parse_historic_quote_ticks
from nautilus_trader.adapters.interactive_brokers.historic import parse_historic_trade_ticks
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestStubs


class TestInteractiveBrokersHistoric:
    def setup(self):
        pass

    def test_parse_historic_trade_ticks(self):
        # Arrange
        raw = IBTestStubs.historic_trades()
        instrument_id = IBTestStubs.instrument(symbol="AAPL").id

        # Act
        ticks = parse_historic_trade_ticks(historic_ticks=raw, instrument_id=instrument_id)

        # Assert
        assert all([isinstance(t, TradeTick) for t in ticks])

        expected = TradeTick.from_dict(
            {
                "type": "TradeTick",
                "instrument_id": "AAPL.NASDAQ",
                "price": "6.2",
                "size": "30.0",
                "aggressor_side": "UNKNOWN",
                "trade_id": "2a62fd894bf039d1907675dcaa8d2a64a9022fe3fa4bdd0ef9972c4b40e041d5",
                "ts_event": 1646185673000000000,
                "ts_init": 1646185673000000000,
            }
        )
        assert ticks[0] == expected

    def test_parse_historic_quote_ticks(self):
        # Arrange
        raw = IBTestStubs.historic_bid_ask()
        instrument_id = IBTestStubs.instrument(symbol="AAPL").id

        # Act
        ticks = parse_historic_quote_ticks(historic_ticks=raw, instrument_id=instrument_id)

        # Assert
        assert all([isinstance(t, QuoteTick) for t in ticks])

        expected = QuoteTick.from_dict(
            {
                "type": "QuoteTick",
                "instrument_id": "AAPL.NASDAQ",
                "bid": "0.99",
                "ask": "15.3",
                "bid_size": "1.0",
                "ask_size": "1.0",
                "ts_event": 1646176203000000000,
                "ts_init": 1646176203000000000,
            }
        )
        assert ticks[0] == expected
