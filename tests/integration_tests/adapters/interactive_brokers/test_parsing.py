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
import datetime

import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.data import bar_spec_to_bar_size
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    ib_contract_to_instrument_id,
)
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    instrument_id_to_ib_contract,
)
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_cash
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_crypto
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_fop
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_fut
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_ind
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import re_opt
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import venues_cash
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import venues_crypto
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import venues_fut
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import venues_opt
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import venues_stk
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.identifiers import InstrumentId
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = IBTestProviderStubs.aapl_instrument()

    @pytest.mark.parametrize(
        "contract, instrument_id",
        [
            # fmt: off
            (IBContract(secType="CASH", exchange="IDEALPRO", localSymbol="EUR.USD"), "EUR/USD.IDEALPRO"),
            (IBContract(secType="OPT", exchange="SMART", localSymbol="AAPL  230217P00155000"), "AAPL230217P00155000.SMART"),
            (IBContract(secType="CONTFUT", exchange="CME", symbol="ES"), "ES.CME"),
            (IBContract(secType="CONTFUT", exchange="CME", symbol="M6E"), "M6E.CME"),
            (IBContract(secType="CONTFUT", exchange="NYMEX", symbol="MCL"), "MCL.NYMEX"),
            (IBContract(secType="CONTFUT", exchange="SNFE", symbol="SPI"), "SPI.SNFE"),
            (IBContract(secType="FUT", exchange="CME", localSymbol="ESH3"), "ESH3.CME"),
            (IBContract(secType="FUT", exchange="CME", localSymbol="M6EH3"), "M6EH3.CME"),
            (IBContract(secType="FUT", exchange="NYMEX", localSymbol="MCLV3"), "MCLV3.NYMEX"),
            (IBContract(secType="FUT", exchange="SNFE", localSymbol="APH3"), "APH3.SNFE"),
            (IBContract(secType="FOP", exchange="NYBOT", localSymbol="EX2G3 P4080"), "EX2G3P4080.NYBOT"),
            (IBContract(secType="FOP", exchange="NYBOT", localSymbol="DXH3 P103.5"), "DXH3P103.5.NYBOT"),
            (IBContract(secType="STK", exchange="SMART", primaryExchange="ARCA", localSymbol="SPY"), "SPY.ARCA"),
            (IBContract(secType="STK", exchange="SMART", primaryExchange="NASDAQ", localSymbol="AAPL"), "AAPL.NASDAQ"),
            (IBContract(secType="STK", exchange="SMART", primaryExchange="NYSE", localSymbol="BF B"), "BF-B.NYSE"),
            (IBContract(secType="STK", exchange="SMART", primaryExchange="ASX", localSymbol="29M"), "29M.ASX"),
            # fmt: on
        ],
    )
    def test_ib_contract_to_instrument_id(self, contract, instrument_id):
        # Arrange, Act
        result = ib_contract_to_instrument_id(contract)

        # Assert
        expected = InstrumentId.from_str(instrument_id)
        assert result == expected

    @pytest.mark.parametrize(
        "instrument_id, contract",
        [
            # fmt: off
            ("EUR/USD.IDEALPRO", IBContract(secType="CASH", exchange="IDEALPRO", localSymbol="EUR.USD")),
            ("AAPL230217P00155000.SMART", IBContract(secType="OPT", exchange="SMART", localSymbol="AAPL  230217P00155000")),
            ("ES.CME", IBContract(secType="CONTFUT", exchange="CME", symbol="ES")),
            ("M6E.CME", IBContract(secType="CONTFUT", exchange="CME", symbol="M6E")),
            ("MCL.NYMEX", IBContract(secType="CONTFUT", exchange="NYMEX", symbol="MCL")),
            ("SPI.SNFE", IBContract(secType="CONTFUT", exchange="SNFE", symbol="SPI")),
            ("ESH3.CME", IBContract(secType="FUT", exchange="CME", localSymbol="ESH3")),
            ("M6EH3.CME", IBContract(secType="FUT", exchange="CME", localSymbol="M6EH3")),
            ("MCLV3.NYMEX", IBContract(secType="FUT", exchange="NYMEX", localSymbol="MCLV3")),
            ("APH3.SNFE", IBContract(secType="FUT", exchange="SNFE", localSymbol="APH3")),
            ("EX2G3P4080.NYBOT", IBContract(secType="FOP", exchange="NYBOT", localSymbol="EX2G3 P4080")),
            ("DXH3P103.5.NYBOT", IBContract(secType="FOP", exchange="NYBOT", localSymbol="DXH3 P103.5")),
            ("SPY.ARCA", IBContract(secType="STK", exchange="SMART", primaryExchange="ARCA", localSymbol="SPY")),
            ("AAPL.NASDAQ", IBContract(secType="STK", exchange="SMART", primaryExchange="NASDAQ", localSymbol="AAPL")),
            ("BF-B.NYSE", IBContract(secType="STK", exchange="SMART", primaryExchange="NYSE", localSymbol="BF B")),
            ("29M.ASX", IBContract(secType="STK", exchange="SMART", primaryExchange="ASX", localSymbol="29M")),
            # fmt: on
        ],
    )
    def test_instrument_id_to_ib_contract(self, instrument_id, contract):
        # Arrange, Act
        result = instrument_id_to_ib_contract(InstrumentId.from_str(instrument_id))

        # Assert
        expected = contract
        assert result == expected

    def test_verified_venues_registered(self):
        # Arrange, Act
        expected_venues_cash = {"IDEALPRO"}
        expected_venues_crypto = {"PAXOS"}
        expected_venues_opt = {"SMART"}
        expected_venues_stk = {"ARCA", "NASDAQ", "NYSE", "AMEX", "ASX"}
        expected_venues_fut = {"CBOT", "CME", "COMEX", "KCBT", "MGE", "NYMEX", "NYBOT", "SNFE"}

        # Assert
        assert len(set(expected_venues_cash) - set(venues_cash)) == 0
        assert len(set(expected_venues_crypto) - set(venues_crypto)) == 0
        assert len(set(expected_venues_opt) - set(venues_opt)) == 0
        assert len(set(expected_venues_stk) - set(venues_stk)) == 0
        assert len(set(expected_venues_fut) - set(venues_fut)) == 0

    def test_regular_expression_forex(self):
        # Arrange
        symbol = "EUR/USD"
        expected = {"symbol": "EUR", "currency": "USD"}

        # Act
        result = re_cash.match(symbol).groupdict()

        # Assert
        assert result == expected

    def test_regular_expression_crypto(self):
        # Arrange
        symbol = "BTC/USD"
        expected = {"symbol": "BTC", "currency": "USD"}

        # Act
        result = re_crypto.match(symbol).groupdict()

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "symbol, expected",
        [
            # fmt: off
            ("AAPL230217P00155000", {"symbol": "AAPL", "expiry": "230217", "right": "P", "strike": "00155", "decimal": "000"}),
            ("A230217P00150000", {"symbol": "A", "expiry": "230217", "right": "P", "strike": "00150", "decimal": "000"}),
            ("CMCSA230217P00039500", {"symbol": "CMCSA", "expiry": "230217", "right": "P", "strike": "00039", "decimal": "500"}),
            # fmt: on
        ],
    )
    def test_regular_expression_option(self, symbol, expected):
        # Arrange, Act
        result = re_opt.match(symbol).groupdict()

        # Act, Assert
        assert result == expected

    @pytest.mark.parametrize(
        "symbol, expected",
        [
            ("ES", {"symbol": "ES"}),
            ("MES", {"symbol": "MES"}),
        ],
    )
    def test_regular_expression_index(self, symbol, expected):
        # Arrange, Act
        result = re_ind.match(symbol).groupdict()

        # Act, Assert
        assert result == expected

    @pytest.mark.parametrize(
        "symbol, expected",
        [
            ("ESH3", {"symbol": "ES", "month": "H", "year": "3"}),
            ("M6EH3", {"symbol": "M6E", "month": "H", "year": "3"}),
        ],
    )
    def test_regular_expression_future(self, symbol, expected):
        # Arrange, Act
        result = re_fut.match(symbol).groupdict()

        # Act, Assert
        assert result == expected

    @pytest.mark.parametrize(
        "symbol, expected",
        [
            # fmt: off
            ("EX2G3P4080", {"symbol": "EX2", "month": "G", "year": "3", "right": "P", "strike": "4080"}),
            ("DXH3P103.5", {"symbol": "DX", "month": "H", "year": "3", "right": "P", "strike": "103.5"}),
            # fmt: on
        ],
    )
    def test_regular_expression_future_options(self, symbol, expected):
        # Arrange, Act
        result = re_fop.match(symbol).groupdict()

        # Assert
        assert result == expected

    # def test_parse_instrument_forex(self):
    # def test_parse_instrument_stock(self):
    # def test_parse_instrument_future(self):
    # def test_parse_instrument_option(self):
    # def test_parse_instrument_crypto(self):

    @pytest.mark.parametrize(
        "bar_spec, expected",
        [
            ("5-SECOND-MID", "5 secs"),
            ("10-SECOND-MID", "10 secs"),
            ("15-SECOND-MID", "15 secs"),
            ("30-SECOND-MID", "30 secs"),
            ("1-MINUTE-MID", "1 min"),
            ("2-MINUTE-MID", "2 mins"),
            ("3-MINUTE-MID", "3 mins"),
            ("5-MINUTE-MID", "5 mins"),
            ("10-MINUTE-MID", "10 mins"),
            ("15-MINUTE-MID", "15 mins"),
            ("20-MINUTE-MID", "20 mins"),
            ("30-MINUTE-MID", "30 mins"),
            ("1-HOUR-MID", "1 hour"),
            ("2-HOUR-MID", "2 hours"),
            ("3-HOUR-MID", "3 hours"),
            ("4-HOUR-MID", "4 hours"),
            ("8-HOUR-MID", "8 hours"),
            ("1-DAY-MID", "1 day"),
            ("1-WEEK-MID", "1 week"),
        ],
    )
    def test_bar_spec_to_bar_size(self, bar_spec, expected):
        # Arrange
        bar_spec = BarSpecification.from_str(bar_spec)

        # Act
        result = bar_spec_to_bar_size(bar_spec)

        # Act, Assert
        assert result == expected

    @pytest.mark.parametrize(
        "timedelta, expected",
        [
            (datetime.timedelta(days=700), "2 Y"),
            (datetime.timedelta(days=365), "1 Y"),
            (datetime.timedelta(days=250), "8 M"),
            (datetime.timedelta(days=30), "1 M"),
            (datetime.timedelta(days=15), "2 W"),
            (datetime.timedelta(days=7), "1 W"),
            (datetime.timedelta(days=1), "1 D"),
            (datetime.timedelta(hours=1), "3600 S"),
            (datetime.timedelta(seconds=60), "60 S"),
            (datetime.timedelta(seconds=15), "30 S"),
        ],
    )
    def test_timedelta_to_duration_str(self, timedelta, expected):
        # Arrange, Act
        result = timedelta_to_duration_str(timedelta)

        # Act, Assert
        assert result == expected
