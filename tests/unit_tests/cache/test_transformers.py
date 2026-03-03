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

from decimal import Decimal

import pytest

from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.cache.transformers import transform_instrument_to_pyo3
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def _perpetual_contract() -> PerpetualContract:
    return PerpetualContract(
        instrument_id=InstrumentId(symbol=Symbol("EURUSD-PERP"), venue=Venue("SIM")),
        raw_symbol=Symbol("EURUSD-PERP"),
        underlying="EURUSD",
        asset_class=AssetClass.FX,
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("USD"),
        is_inverse=False,
        price_precision=5,
        size_precision=0,
        price_increment=Price.from_str("0.00001"),
        size_increment=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0004"),
    )


# All instrument types with their factory functions
ALL_INSTRUMENTS = [
    pytest.param(TestInstrumentProvider.betting_instrument, id="BettingInstrument"),
    pytest.param(TestInstrumentProvider.binary_option, id="BinaryOption"),
    pytest.param(TestInstrumentProvider.audusd_cfd, id="Cfd"),
    pytest.param(TestInstrumentProvider.commodity, id="Commodity"),
    pytest.param(TestInstrumentProvider.btcusdt_future_binance, id="CryptoFuture"),
    pytest.param(TestInstrumentProvider.crypto_option, id="CryptoOption"),
    pytest.param(TestInstrumentProvider.btcusdt_perp_binance, id="CryptoPerpetual"),
    pytest.param(TestInstrumentProvider.ethusdt_binance, id="CurrencyPair"),
    pytest.param(TestInstrumentProvider.equity, id="Equity"),
    pytest.param(TestInstrumentProvider.future, id="FuturesContract"),
    pytest.param(TestInstrumentProvider.futures_spread, id="FuturesSpread"),
    pytest.param(TestInstrumentProvider.index_instrument, id="IndexInstrument"),
    pytest.param(TestInstrumentProvider.aapl_option, id="OptionContract"),
    pytest.param(TestInstrumentProvider.option_spread, id="OptionSpread"),
    pytest.param(_perpetual_contract, id="PerpetualContract"),
]

# Types that have Cython from_pyo3 methods (BettingInstrument and BinaryOption do not)
ROUND_TRIP_INSTRUMENTS = [
    pytest.param(TestInstrumentProvider.audusd_cfd, id="Cfd"),
    pytest.param(TestInstrumentProvider.commodity, id="Commodity"),
    pytest.param(TestInstrumentProvider.btcusdt_future_binance, id="CryptoFuture"),
    pytest.param(TestInstrumentProvider.crypto_option, id="CryptoOption"),
    pytest.param(TestInstrumentProvider.btcusdt_perp_binance, id="CryptoPerpetual"),
    pytest.param(TestInstrumentProvider.ethusdt_binance, id="CurrencyPair"),
    pytest.param(TestInstrumentProvider.equity, id="Equity"),
    pytest.param(TestInstrumentProvider.future, id="FuturesContract"),
    pytest.param(TestInstrumentProvider.futures_spread, id="FuturesSpread"),
    pytest.param(TestInstrumentProvider.index_instrument, id="IndexInstrument"),
    pytest.param(TestInstrumentProvider.aapl_option, id="OptionContract"),
    pytest.param(TestInstrumentProvider.option_spread, id="OptionSpread"),
    pytest.param(_perpetual_contract, id="PerpetualContract"),
]


class TestTransformInstrumentToPyo3:
    @pytest.mark.parametrize("factory", ALL_INSTRUMENTS)
    def test_to_pyo3(self, factory):
        instrument = factory()
        result = transform_instrument_to_pyo3(instrument)
        assert result.id.value == instrument.id.value

    @pytest.mark.parametrize("factory", ROUND_TRIP_INSTRUMENTS)
    def test_round_trip(self, factory):
        instrument = factory()
        pyo3_instrument = transform_instrument_to_pyo3(instrument)
        result = transform_instrument_from_pyo3(pyo3_instrument)
        assert result.id == instrument.id
        assert type(result) is type(instrument)

    def test_from_pyo3_none_returns_none(self):
        assert transform_instrument_from_pyo3(None) is None

    def test_to_pyo3_unknown_type_raises(self):
        with pytest.raises(ValueError, match="Unknown instrument type"):
            transform_instrument_to_pyo3("not_an_instrument")

    def test_from_pyo3_unknown_type_raises(self):
        with pytest.raises(ValueError, match="Unknown instrument type"):
            transform_instrument_from_pyo3("not_an_instrument")
