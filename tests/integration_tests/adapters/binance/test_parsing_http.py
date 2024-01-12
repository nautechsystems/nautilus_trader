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

import pkgutil

import msgspec
import pytest

from nautilus_trader.adapters.binance.common.schemas.market import BinanceDepth
from nautilus_trader.test_kit.providers import TestInstrumentProvider


pytestmark = pytest.mark.skip(reason="Repair order book parsing")

ETHUSDT = TestInstrumentProvider.ethusdt_binance()


class TestBinanceHttpParsing:
    def test_parse_book_snapshot(self):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_depth.json",
        )

        # Act
        decoder = msgspec.json.Decoder(BinanceDepth)
        data = decoder.decode(raw)
        result = data.parse_to_order_book_snapshot(
            instrument_id=ETHUSDT.id,
            ts_init=2,
        )

        # Assert
        assert result.instrument_id == ETHUSDT.id
        assert result.asks == [
            [60650.01, 0.61982],
            [60653.68, 0.00696],
            [60653.69, 0.00026],
            [60656.89, 0.01],
            [60657.87, 0.02],
            [60657.99, 0.04993],
            [60658.0, 0.02],
            [60659.0, 0.12244],
            [60659.71, 0.35691],
            [60659.94, 0.9617],
        ]
        assert result.bids == [
            [60650.0, 0.00213],
            [60648.08, 0.06346],
            [60648.01, 0.0643],
            [60648.0, 0.09332],
            [60647.53, 0.19622],
            [60647.52, 0.03],
            [60646.55, 0.06431],
            [60643.57, 0.08904],
            [60643.56, 0.00203],
            [60639.93, 0.07282],
        ]
        assert result.sequence == 14527958487
        assert result.ts_init == 2
