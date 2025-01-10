# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentOption
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsLinearResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsOptionResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentSpot
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsSpotResponse


fee_rate: BybitFeeRate = BybitFeeRate(
    symbol="BTC-29MAR24-100000-C-OPTION",
    takerFeeRate="0.0006",
    makerFeeRate="0.0001",
)


class TestBybitInstruments:
    def setup(self) -> None:
        # linear
        linear_data: BybitInstrumentsLinearResponse = msgspec.json.Decoder(
            BybitInstrumentsLinearResponse,
        ).decode(
            pkgutil.get_data(  # type: ignore [arg-type]
                "tests.integration_tests.adapters.bybit.resources.http_responses.linear",
                "instruments.json",
            ),
        )
        self.linear_instrument: BybitInstrumentLinear = linear_data.result.list[0]
        # spot
        spot_data: BybitInstrumentsSpotResponse = msgspec.json.Decoder(
            BybitInstrumentsSpotResponse,
        ).decode(
            pkgutil.get_data(  # type: ignore [arg-type]
                "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
                "instruments.json",
            ),
        )
        self.spot_instrument: BybitInstrumentSpot = spot_data.result.list[0]
        # option
        option_data: BybitInstrumentsOptionResponse = msgspec.json.Decoder(
            BybitInstrumentsOptionResponse,
        ).decode(
            pkgutil.get_data(  # type: ignore [arg-type]
                "tests.integration_tests.adapters.bybit.resources.http_responses.option",
                "instruments.json",
            ),
        )
        self.option_instrument: BybitInstrumentOption = option_data.result.list[0]

    # def test_parse_to_instrument_option(self):
    #     option = self.option_instrument.parse_to_instrument(fee_rate)
    #     assert option.symbol.value == 'BTC-29MAR24-100000-C-OPTION'
    #     assert str(option.id) == 'BTC-29MAR24-100000-C-OPTION.BYBIT'
    #     assert option.asset_class == AssetClass.CRYPTOCURRENCY
