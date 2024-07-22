# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVenue:
    def test_instrument_status(self):
        # Arrange
        update = InstrumentStatus(
            instrument_id=InstrumentId.from_str("MSFT.XNAS"),
            action=MarketStatusAction.TRADING,
            ts_event=0,
            ts_init=0,
            is_trading=True,
            is_quoting=True,
            is_short_sell_restricted=False,
        )

        # Act, Assert
        assert InstrumentStatus.from_dict(InstrumentStatus.to_dict(update)) == update
        assert (
            repr(update)
            == "InstrumentStatus(instrument_id=MSFT.XNAS, action=TRADING, reason=None, trading_event=None, is_trading=True, is_quoting=True, is_short_sell_restricted=False, ts_event=0)"  # noqa: E501
        )

    def test_instrument_close(self):
        # Arrange
        update = InstrumentClose(
            instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
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
