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

import datetime
from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.parsing.data import bar_spec_to_bar_size
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_CASH
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_CRYPTO
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_FOP
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_FOP_ORIGINAL
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_FUT
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_FUT_ORIGINAL
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_FUT_UNDERLYING
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import RE_OPT
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import VENUES_CASH
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import VENUES_CRYPTO
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import VENUES_FUT
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import VENUES_OPT
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import _tick_size_to_precision
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import expiry_timestring_to_datetime
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import OptionContract
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


simplified_symbology_params = [
    (IBContract(secType="CASH", exchange="IDEALPRO", localSymbol="EUR.USD"), "EUR/USD.IDEALPRO"),
    (IBContract(secType="OPT", exchange="SMART", localSymbol="AAPL  230217P00155000"), "AAPL230217P00155000.SMART"),
    (IBContract(secType="CONTFUT", exchange="CME", symbol="ES"), "ES.CME"),
    (IBContract(secType="CONTFUT", exchange="CME", symbol="M6E"), "M6E.CME"),
    (IBContract(secType="CONTFUT", exchange="NYMEX", symbol="MCL"), "MCL.NYMEX"),
    (IBContract(secType="CONTFUT", exchange="SNFE", symbol="SPI"), "SPI.SNFE"),
    (IBContract(secType="FUT", exchange="CME", localSymbol="ESH3"), "ESH3.CME"),
    (IBContract(secType="FUT", exchange="CME", localSymbol="M6EH3"), "M6EH3.CME"),
    (IBContract(secType="FUT", exchange="CBOT", localSymbol="MYMM3"), "MYMM3.CBOT"),
    (IBContract(secType="FUT", exchange="NYMEX", localSymbol="MCLV3"), "MCLV3.NYMEX"),
    (IBContract(secType="FUT", exchange="SNFE", localSymbol="APH3"), "APH3.SNFE"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="EX2G3 P4080"), "EX2G3 P4080.NYBOT"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="DXH3 P103.5"), "DXH3 P103.5.NYBOT"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="ARCA", localSymbol="SPY"), "SPY.ARCA"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="NASDAQ", localSymbol="AAPL"), "AAPL.NASDAQ"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="NYSE", localSymbol="BF B"), "BF-B.NYSE"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="ASX", localSymbol="29M"), "29M.ASX"),
    (IBContract(secType="CRYPTO", exchange="PAXOS", localSymbol="BTC.USD"), "BTC/USD.PAXOS"),
    (IBContract(secType="IND", exchange="CBOE", localSymbol="SPX"), "^SPX.CBOE"),
    (IBContract(secType="IND", exchange="ASX", localSymbol="XJO"), "^XJO.ASX"),
]



raw_symbology_params = [
    (IBContract(secType="CASH", exchange="IDEALPRO", localSymbol="EUR.USD"), "EUR.USD=CASH.IDEALPRO"),
    (IBContract(secType="OPT", exchange="SMART", localSymbol="AAPL  230217P00155000"), "AAPL  230217P00155000=OPT.SMART"),
    (IBContract(secType="FUT", exchange="CME", localSymbol="ESH3"), "ESH3=FUT.CME"),
    (IBContract(secType="FUT", exchange="CME", localSymbol="M6EH3"), "M6EH3=FUT.CME"),
    (IBContract(secType="FUT", exchange="CBOT", localSymbol="MYMM3"), "MYMM3=FUT.CBOT"),
    (IBContract(secType="FUT", exchange="NYMEX", localSymbol="MCLV3"), "MCLV3=FUT.NYMEX"),
    (IBContract(secType="FUT", exchange="SNFE", localSymbol="APH3"), "APH3=FUT.SNFE"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="EX2G3 P4080"), "EX2G3 P4080=FOP.NYBOT"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="DXH3 P103.5"), "DXH3 P103.5=FOP.NYBOT"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="ARCA", localSymbol="SPY"), "SPY=STK.ARCA"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="NASDAQ", localSymbol="AAPL"), "AAPL=STK.NASDAQ"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="NYSE", localSymbol="BF B"), "BF B=STK.NYSE"),
    (IBContract(secType="STK", exchange="SMART", primaryExchange="ASX", localSymbol="29M"), "29M=STK.ASX"),
    (IBContract(secType="FUT", exchange="EUREX", localSymbol="SCOI 20251219 M"), "SCOI 20251219 M=FUT.EUREX"),
    (IBContract(secType="FUT", exchange="LMEOTC", localSymbol="AH_20240221"), "AH_20240221=FUT.LMEOTC"),
    (IBContract(secType="FUT", exchange="NSE", localSymbol="INFY24FEBFUT"), "INFY24FEBFUT=FUT.NSE"),
    (IBContract(secType="FUT", exchange="OMS", localSymbol="4TLSN4L"), "4TLSN4L=FUT.OMS"),
    (IBContract(secType="FUT", exchange="OMS", localSymbol="3TLSN4N"), "3TLSN4N=FUT.OMS"),
    (IBContract(secType="FUT", exchange="MEFFRV", localSymbol="M3FIDRM4P"), "M3FIDRM4P=FUT.MEFFRV"),
    (IBContract(secType="FUT", exchange="MEXDER", localSymbol="DVCE91MR24"), "DVCE91MR24=FUT.MEXDER"),
    (IBContract(secType="FUT", exchange="MEXDER", localSymbol="DVCXC MR24"), "DVCXC MR24=FUT.MEXDER"),
    (IBContract(secType="FUT", exchange="MEXDER", localSymbol="DVM3  JN24"), "DVM3  JN24=FUT.MEXDER"),
    (IBContract(secType="FUT", exchange="CDE", localSymbol="SXAH24"), "SXAH24=FUT.CDE"),
    (IBContract(secType="FUT", exchange="IPE", localSymbol="HOILN7"), "HOILN7=FUT.IPE"),
    (IBContract(secType="FUT", exchange="CFE", localSymbol="IBHYH4"), "IBHYH4=FUT.CFE"),
    (IBContract(secType="FUT", exchange="IDEM", localSymbol="ISP   24L20"), "ISP   24L20=FUT.IDEM"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="EX2G3 P4080"), "EX2G3 P4080=FOP.NYBOT"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="DXH3 P103.5"), "DXH3 P103.5=FOP.NYBOT"),
    (IBContract(secType="FOP", exchange="CME", localSymbol="6NZ4 P0655"), "6NZ4 P0655=FOP.CME"),
    (IBContract(secType="FOP", exchange="EUREX", localSymbol="C OEXD 20261218 50 M"), "C OEXD 20261218 50 M=FOP.EUREX"),
    (IBContract(secType="FOP", exchange="IPE", localSymbol="WTIF5 C80"), "WTIF5 C80=FOP.IPE"),
    (IBContract(secType="FOP", exchange="MEXDER", localSymbol="DVIP40000L"), "DVIP40000L=FOP.MEXDER"),
    (IBContract(secType="FOP", exchange="NYBOT", localSymbol="OJF6 C1.3"), "OJF6 C1.3=FOP.NYBOT"),
    (IBContract(secType="FOP", exchange="SGX", localSymbol="FCHZ24_C7000"), "FCHZ24_C7000=FOP.SGX"),
    (IBContract(secType="FUT", exchange="EUREX", localSymbol="FMEU 20240125 D"), "FMEU 20240125 D=FUT.EUREX"),
    (IBContract(secType="FUT", exchange="EUREX", localSymbol="FMEU 20240126 D"), "FMEU 20240126 D=FUT.EUREX"),
    (IBContract(secType="FUT", exchange="EUREX", localSymbol="FMEU 20240129 D"), "FMEU 20240129 D=FUT.EUREX"),
    (IBContract(secType="FOP", exchange="ENDEX", localSymbol="TFMG0"), "TFMG0=FOP.ENDEX"),
    (IBContract(secType="FUT", exchange="OSE.JPN", localSymbol="1690200A1"), "1690200A1=FUT.OSE/JPN"),
    (IBContract(secType="CRYPTO", exchange="PAXOS", localSymbol="BTC.USD"), "BTC.USD=CRYPTO.PAXOS"),
    (IBContract(secType="IND", exchange="CBOE", localSymbol="SPX"), "SPX=IND.CBOE"),
    (IBContract(secType="IND", exchange="ASX", localSymbol="XJO"), "XJO=IND.ASX"),
]



@pytest.mark.parametrize("contract, instrument_id", simplified_symbology_params)
def test_ib_contract_to_instrument_id_simplified_symbology(contract, instrument_id):
    # Arrange, Act
    instrument_id = InstrumentId.from_str(instrument_id)
    result = ib_contract_to_instrument_id(contract, instrument_id.venue.value)

    # Assert
    assert result == instrument_id


@pytest.mark.parametrize("contract, instrument_id", raw_symbology_params)
def test_ib_contract_to_instrument_id_raw_symbology(contract, instrument_id):
    # Arrange, Act
    instrument_id_type = InstrumentId.from_str(instrument_id)
    result = ib_contract_to_instrument_id(
        contract,
        instrument_id_type.venue.value,
        SymbologyMethod.IB_RAW,
    )

    # Assert
    expected = instrument_id_type
    assert result == expected


@pytest.mark.parametrize("contract, instrument_id", simplified_symbology_params)
def test_instrument_id_to_ib_contract_simplified_symbology(instrument_id, contract):
    # Arrange, Act
    instrument_id_type = InstrumentId.from_str(instrument_id)
    result = instrument_id_to_ib_contract(instrument_id_type, instrument_id_type.venue.value)

    # Assert
    expected = contract
    assert result == expected


@pytest.mark.parametrize("contract, instrument_id", raw_symbology_params)
def test_instrument_id_to_ib_contract_raw_symbology(instrument_id, contract):
    # Arrange, Act
    instrument_id = InstrumentId.from_str(instrument_id)
    result = instrument_id_to_ib_contract(
        instrument_id,
        instrument_id.venue.value,
        SymbologyMethod.IB_RAW,
    )

    # Assert
    expected = contract
    assert result == expected


def test_verified_venues_registered():
    # Arrange, Act
    expected_venues_cash = {"IDEALPRO"}
    expected_venues_crypto = {"PAXOS"}
    expected_venues_opt = {"SMART"}
    expected_venues_fut = {"CBOT", "CME", "COMEX", "KCBT", "MGE", "NYMEX", "NYBOT", "SNFE"}

    # Assert
    assert len(set(expected_venues_cash) - set(VENUES_CASH)) == 0
    assert len(set(expected_venues_crypto) - set(VENUES_CRYPTO)) == 0
    assert len(set(expected_venues_opt) - set(VENUES_OPT)) == 0
    assert len(set(expected_venues_fut) - set(VENUES_FUT)) == 0


def test_regular_expression_forex():
    # Arrange
    symbol = "EUR/USD"
    expected = {"symbol": "EUR", "currency": "USD"}

    # Act
    result = RE_CASH.match(symbol).groupdict()

    # Assert
    assert result == expected


def test_regular_expression_crypto():
    # Arrange
    symbol = "BTC/USD"
    expected = {"symbol": "BTC", "currency": "USD"}

    # Act
    result = RE_CRYPTO.match(symbol).groupdict()

    # Assert
    assert result == expected



@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("AAPL230217P00155000", {"symbol": "AAPL", "expiry": "230217", "right": "P", "strike": "00155", "decimal": "000"}),
        ("A230217P00150000", {"symbol": "A", "expiry": "230217", "right": "P", "strike": "00150", "decimal": "000"}),
        ("CMCSA230217P00039500", {"symbol": "CMCSA", "expiry": "230217", "right": "P", "strike": "00039", "decimal": "500"}),
    ],
)
def test_regular_expression_option(symbol, expected):
    # Arrange, Act
    result = RE_OPT.match(symbol).groupdict()

    # Act, Assert
    assert result == expected



@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("ES", {"symbol": "ES"}),
        ("MES", {"symbol": "MES"}),
    ],
)
def test_regular_expression_index(symbol, expected):
    # Arrange, Act
    result = RE_FUT_UNDERLYING.match(symbol).groupdict()

    # Act, Assert
    assert result == expected


@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("ESH23", {"symbol": "ES", "month": "H", "year": "23"}),
        ("M6EH23", {"symbol": "M6E", "month": "H", "year": "23"}),
    ],
)
def test_regular_expression_future(symbol, expected):
    # Arrange, Act
    result = RE_FUT.match(symbol).groupdict()

    # Act, Assert
    assert result == expected


@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("ESH3", {"symbol": "ES", "month": "H", "year": "3"}),
        ("M6EH3", {"symbol": "M6E", "month": "H", "year": "3"}),
    ],
)
def test_regular_expression_future_original(symbol, expected):
    # Arrange, Act
    result = RE_FUT_ORIGINAL.match(symbol).groupdict()

    # Act, Assert
    assert result == expected



@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("EX2G23P4080", {"symbol": "EX2", "month": "G", "year": "23", "right": "P", "strike": "4080"}),
        ("DXH23P103.5", {"symbol": "DX", "month": "H", "year": "23", "right": "P", "strike": "103.5"}),
    ],
)
def test_regular_expression_future_options(symbol, expected):
    # Arrange, Act
    result = RE_FOP.match(symbol).groupdict()

    # Assert
    assert result == expected




@pytest.mark.parametrize(
    ("symbol", "expected"),
    [
        ("EX2G3 P4080", {"symbol": "EX2", "month": "G", "year": "3", "right": "P", "strike": "4080"}),
        ("DXH3 P103.5", {"symbol": "DX", "month": "H", "year": "3", "right": "P", "strike": "103.5"}),
    ],
)
def test_regular_expression_future_options_original(symbol, expected):
    # Arrange, Act
    result = RE_FOP_ORIGINAL.match(symbol).groupdict()

    # Assert
    assert result == expected



@pytest.mark.parametrize(
    ("bar_spec", "expected"),
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
def test_bar_spec_to_bar_size(bar_spec, expected):
    # Arrange
    bar_spec = BarSpecification.from_str(bar_spec)

    # Act
    result = bar_spec_to_bar_size(bar_spec)

    # Act, Assert
    assert result == expected


@pytest.mark.parametrize(
    ("timedelta", "expected"),
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
def test_timedelta_to_duration_str(timedelta, expected):
    # Arrange, Act
    result = timedelta_to_duration_str(timedelta)

    # Act, Assert
    assert result == expected


@pytest.mark.parametrize(
    ("tick_size", "expected"),
    [
        (5e-10, 10),
        (5e-07, 7),
        (5e-05, 5),
        (Decimal("0.01"), 2),
        (Decimal("1E-8"), 8),
    ],
)
def test_tick_size_to_precision(tick_size: float | Decimal, expected: int):
    # Arrange, Act
    result = _tick_size_to_precision(tick_size)

    # Act, Assert
    assert result == expected


@pytest.mark.parametrize(
    ("expiry", "expected"),
    [
        ("20240411", pd.Timestamp("2024-04-11 00:00:00", tz="UTC")),
        ("20250420 18:35:00 GB", pd.Timestamp("2025-04-20 17:35:00", tz="UTC")),
    ],
)
def test_expiry_timestring_to_datetime(expiry: str, expected: pd.Timestamp):
    # Arrange, Act
    result = expiry_timestring_to_datetime(expiry=expiry)
    # Act, Assert
    assert result == expected


def test_parse_instrument_future_option():
    # Arrange
    contract_details = IBTestContractStubs.es_future_option_contract_details()
    contract_details.contract = IBContract(**contract_details.contract.__dict__)
    contract_details = IBContractDetails(**contract_details.__dict__)

    # Act
    instrument = parse_instrument(contract_details, "CME")

    # Assert
    assert instrument.id.value == "E4AN4 C5655.CME"
    assert isinstance(instrument, OptionContract), "instrument is not a OptionContract"
