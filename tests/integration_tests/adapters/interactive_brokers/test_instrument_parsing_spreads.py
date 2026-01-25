#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-5 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Comprehensive tests for Interactive Brokers instrument parsing, especially spread
instruments.
"""

import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_futures_spread
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    parse_futures_spread_instrument_id,
)
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_option_spread
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    parse_option_spread_instrument_id,
)
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import new_generic_spread_id
from nautilus_trader.model.instruments import FuturesSpread
from nautilus_trader.model.instruments import OptionSpread
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class TestSpreadInstrumentParsing:
    """
    Test cases for parsing spread instruments from instrument IDs.
    """

    def test_parse_option_spread_instrument_id_basic_spread(self):
        """
        Test parsing basic 1x1 spread instrument ID.
        """
        # Create spread instrument ID
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg1_details = IBContractDetails(
            contract=leg1_contract,
            minTick=0.01,
            underSymbol="SPY",
        )

        leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg2_details = IBContractDetails(
            contract=leg2_contract,
            minTick=0.01,
            underSymbol="SPY",
        )

        leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

        # Parse the spread
        instrument = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        # Verify the result
        assert isinstance(instrument, OptionSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "SPY"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(100)
        assert instrument.lot_size == Quantity.from_int(100)  # Should equal multiplier
        assert instrument.price_increment == Price.from_str("0.01")

    def test_parse_option_spread_instrument_id_ratio_spread(self):
        """
        Test parsing ratio spread instrument ID.
        """
        leg1_id = InstrumentId.from_str("E4DN5 P6350.XCME")
        leg2_id = InstrumentId.from_str("E4DN5 P6355.XCME")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -2)])

        # Create mock contract details for futures options legs
        leg1_contract = IBContract(secType="FOP", symbol="ES", currency="USD", multiplier="50")
        leg1_details = IBContractDetails(
            contract=leg1_contract,
            minTick=0.05,
            underSymbol="ES",
        )

        leg2_contract = IBContract(secType="FOP", symbol="ES", currency="USD", multiplier="50")
        leg2_details = IBContractDetails(
            contract=leg2_contract,
            minTick=0.05,
            underSymbol="ES",
        )

        leg_contract_details = [(leg1_details, 1), (leg2_details, -2)]

        instrument = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "ES"  # ES futures options
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(50)
        assert instrument.lot_size == Quantity.from_int(50)  # Should equal multiplier
        assert instrument.price_increment == Price.from_str("0.05")

    def test_parse_option_spread_instrument_id_butterfly(self):
        """
        Test parsing butterfly spread (3 legs).
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C405.SMART")
        leg3_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -2), (leg3_id, 1)])

        # Create mock contract details for legs
        leg_contract_details = []
        for _leg_id, ratio in [(leg1_id, 1), (leg2_id, -2), (leg3_id, 1)]:
            contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
            details = IBContractDetails(contract=contract, minTick=0.01, underSymbol="SPY")
            leg_contract_details.append((details, ratio))

        instrument = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def test_parse_option_spread_instrument_id_iron_condor(self):
        """
        Test parsing iron condor spread (4 legs).
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C405.SMART")
        leg3_id = InstrumentId.from_str("SPY P395.SMART")
        leg4_id = InstrumentId.from_str("SPY P390.SMART")
        spread_id = new_generic_spread_id(
            [
                (leg1_id, 1),
                (leg2_id, -1),
                (leg3_id, 1),
                (leg4_id, -1),
            ],
        )

        # Create mock contract details for legs
        leg_contract_details = []
        for _leg_id, ratio in [(leg1_id, 1), (leg2_id, -1), (leg3_id, 1), (leg4_id, -1)]:
            contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
            details = IBContractDetails(contract=contract, minTick=0.01, underSymbol="SPY")
            leg_contract_details.append((details, ratio))

        instrument = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def test_parse_option_spread_instrument_id_invalid(self):
        """
        Test parsing invalid spread instrument ID.
        """
        # Create invalid spread ID (no legs)
        invalid_id = InstrumentId.from_str("INVALID.SMART")

        with pytest.raises(ValueError, match="leg_contract_details must be provided"):
            parse_option_spread_instrument_id(invalid_id, [])


class TestOptionSpreadParsing:
    """
    Test cases for parsing option spread contracts (IB BAG contracts).
    """

    def test_parse_option_spread_basic(self):
        """
        Test parsing basic option spread contract.
        """
        # Create mock BAG contract details
        contract_details = self._create_bag_contract_details(
            symbol="SPY",
            currency="USD",
            multiplier="100",
            min_tick=0.01,
            combo_legs_descrip="SPY C400/C410 SPREAD",
        )
        instrument_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )
        instrument = parse_option_spread(contract_details, instrument_id)

        assert isinstance(instrument, OptionSpread)
        assert instrument.id == instrument_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "SPY"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.price_increment == Price(0.01, 2)

    def test_parse_option_spread_iron_condor(self):
        """
        Test parsing iron condor option spread contract.
        """
        contract_details = self._create_bag_contract_details(
            symbol="SPY",
            currency="USD",
            multiplier="100",
            min_tick=0.01,
            combo_legs_descrip="SPY IRON CONDOR 400/405/395/390",
        )

        instrument_id = InstrumentId.from_str("IRON_CONDOR_SPY.SMART")

        instrument = parse_option_spread(contract_details, instrument_id)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def test_parse_option_spread_butterfly(self):
        """
        Test parsing butterfly option spread contract.
        """
        contract_details = self._create_bag_contract_details(
            symbol="SPY",
            currency="USD",
            multiplier="100",
            min_tick=0.01,
            combo_legs_descrip="SPY BUTTERFLY 400/405/410",
        )

        instrument_id = InstrumentId.from_str("BUTTERFLY_SPY.SMART")

        instrument = parse_option_spread(contract_details, instrument_id)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def _create_bag_contract_details(
        self,
        symbol: str,
        currency: str,
        multiplier: str,
        min_tick: float,
        combo_legs_descrip: str = "",
        under_symbol: str | None = None,
    ) -> IBContractDetails:
        """
        Create mock BAG contract details for testing.
        """
        contract = IBContract(
            secType="BAG",
            symbol=symbol,
            currency=currency,
            exchange="SMART",
            multiplier=multiplier,
            localSymbol=f"{symbol}_BAG",
            comboLegsDescrip=combo_legs_descrip,
        )

        contract_details = IBContractDetails(
            contract=contract,
            minTick=min_tick,
            underSymbol=under_symbol or symbol,
        )

        return contract_details


# Test for basic functionality
def test_spread_instrument_parsing_integration():
    # Test basic functionality
    leg1_id = InstrumentId.from_str("SPY C400.SMART")
    leg2_id = InstrumentId.from_str("SPY C410.SMART")
    spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

    # Create mock contract details
    leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
    leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

    leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
    leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.01, underSymbol="SPY")

    leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

    instrument = parse_option_spread_instrument_id(spread_id, leg_contract_details)

    assert isinstance(instrument, OptionSpread)
    assert instrument.id == spread_id

    print("Spread instrument parsing working correctly")


class TestFuturesSpreadInstrumentParsing:
    """
    Test cases for parsing futures spread instruments from instrument IDs.
    """

    def test_parse_futures_spread_instrument_id_calendar_spread(self):
        """
        Test parsing basic calendar spread (1x1) for futures.
        """
        # Create spread instrument ID for ES futures calendar spread
        leg1_id = InstrumentId.from_str("ESZ4.XCME")
        leg2_id = InstrumentId.from_str("ESH5.XCME")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg1_details = IBContractDetails(
            contract=leg1_contract,
            minTick=0.25,
            underSymbol="ES",
        )

        leg2_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg2_details = IBContractDetails(
            contract=leg2_contract,
            minTick=0.25,
            underSymbol="ES",
        )

        leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

        # Parse the spread
        instrument = parse_futures_spread_instrument_id(spread_id, leg_contract_details)

        # Verify the result
        assert isinstance(instrument, FuturesSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "ES"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(50)
        assert instrument.lot_size == Quantity.from_int(1)  # For futures, lot size is typically 1
        assert instrument.price_increment == Price.from_str("0.25")

    def test_parse_futures_spread_instrument_id_ratio_spread(self):
        """
        Test parsing ratio spread for futures.
        """
        leg1_id = InstrumentId.from_str("NQZ4.XCME")
        leg2_id = InstrumentId.from_str("NQH5.XCME")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -2)])

        # Create mock contract details for futures legs
        leg1_contract = IBContract(secType="FUT", symbol="NQ", currency="USD", multiplier="20")
        leg1_details = IBContractDetails(
            contract=leg1_contract,
            minTick=0.25,
            underSymbol="NQ",
        )

        leg2_contract = IBContract(secType="FUT", symbol="NQ", currency="USD", multiplier="20")
        leg2_details = IBContractDetails(
            contract=leg2_contract,
            minTick=0.25,
            underSymbol="NQ",
        )

        leg_contract_details = [(leg1_details, 1), (leg2_details, -2)]

        instrument = parse_futures_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, FuturesSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "NQ"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(20)
        assert instrument.lot_size == Quantity.from_int(1)  # For futures, lot size is typically 1
        assert instrument.price_increment == Price.from_str("0.25")

    def test_parse_futures_spread_instrument_id_crack_spread(self):
        """
        Test parsing crack spread (3 legs) for futures.
        """
        leg1_id = InstrumentId.from_str("CLZ4.NYMEX")
        leg2_id = InstrumentId.from_str("RBZ4.NYMEX")
        leg3_id = InstrumentId.from_str("HOZ4.NYMEX")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1), (leg3_id, -1)])

        # Create mock contract details for legs
        leg_contract_details = []
        for _leg_id, ratio, symbol in [
            (leg1_id, 1, "CL"),
            (leg2_id, -1, "RB"),
            (leg3_id, -1, "HO"),
        ]:
            contract = IBContract(secType="FUT", symbol=symbol, currency="USD", multiplier="1000")
            details = IBContractDetails(contract=contract, minTick=0.01, underSymbol=symbol)
            leg_contract_details.append((details, ratio))

        instrument = parse_futures_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, FuturesSpread)
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "CL"  # Uses first leg's underlying
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(1000)

    def test_parse_futures_spread_instrument_id_invalid(self):
        """
        Test parsing invalid futures spread instrument ID.
        """
        # Create invalid spread ID (no legs)
        invalid_id = InstrumentId.from_str("INVALID.XCME")

        with pytest.raises(ValueError, match="leg_contract_details must be provided"):
            parse_futures_spread_instrument_id(invalid_id, [])


class TestFuturesSpreadParsing:
    """
    Test cases for parsing futures spread contracts (IB BAG contracts).
    """

    def test_parse_futures_spread_calendar_spread(self):
        """
        Test parsing basic futures calendar spread contract.
        """
        # Create mock BAG contract details
        contract_details = self._create_bag_contract_details(
            symbol="ES",
            currency="USD",
            multiplier="50",
            min_tick=0.25,
            combo_legs_descrip="ES DEC24/MAR25 SPREAD",
            under_symbol="ES",
        )
        instrument_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("ESZ4.XCME"), 1),
                (InstrumentId.from_str("ESH5.XCME"), -1),
            ],
        )
        instrument = parse_futures_spread(contract_details, instrument_id)

        assert isinstance(instrument, FuturesSpread)
        assert instrument.id == instrument_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "ES"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.price_increment == Price(0.25, 2)
        assert instrument.multiplier == Quantity.from_int(50)
        assert instrument.lot_size == Quantity.from_int(1)  # For futures, lot size is typically 1

    def test_parse_futures_spread_crack_spread(self):
        """
        Test parsing crack spread futures contract.
        """
        contract_details = self._create_bag_contract_details(
            symbol="CL",
            currency="USD",
            multiplier="1000",
            min_tick=0.01,
            combo_legs_descrip="CL/RB/HO CRACK SPREAD",
            under_symbol="CL",
        )

        instrument_id = InstrumentId.from_str("CRACK_SPREAD_CL.SMART")

        instrument = parse_futures_spread(contract_details, instrument_id)

        assert isinstance(instrument, FuturesSpread)
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "CL"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(1000)

    def test_parse_futures_spread_calendar_spread_different_underlying(self):
        """
        Test parsing calendar spread with different underlying symbols.
        """
        contract_details = self._create_bag_contract_details(
            symbol="NQ",
            currency="USD",
            multiplier="20",
            min_tick=0.25,
            combo_legs_descrip="NQ DEC24/MAR25 SPREAD",
            under_symbol="NQ",
        )
        instrument_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("NQZ4.XCME"), 1),
                (InstrumentId.from_str("NQH5.XCME"), -1),
            ],
        )
        instrument = parse_futures_spread(contract_details, instrument_id)

        assert isinstance(instrument, FuturesSpread)
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "NQ"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(20)

    def _create_bag_contract_details(
        self,
        symbol: str,
        currency: str,
        multiplier: str,
        min_tick: float,
        combo_legs_descrip: str = "",
        under_symbol: str | None = None,
    ) -> IBContractDetails:
        """
        Create mock BAG contract details for testing futures spreads.
        """
        contract = IBContract(
            secType="BAG",
            symbol=symbol,
            currency=currency,
            exchange="XCME",
            multiplier=multiplier,
            localSymbol=f"{symbol}_BAG",
            comboLegsDescrip=combo_legs_descrip,
        )

        contract_details = IBContractDetails(
            contract=contract,
            minTick=min_tick,
            underSymbol=under_symbol or symbol,
        )

        return contract_details


class TestSpreadLegsMethod:
    """
    Test cases for the legs() method on spread instruments.
    """

    def test_option_spread_legs_with_generic_spread_id(self):
        """
        Test that legs() method correctly parses generic spread IDs for option spreads.
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

        leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.01, underSymbol="SPY")

        leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

        # Create option spread with generic spread ID
        option_spread = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        legs = option_spread.legs()

        assert len(legs) == 2
        assert legs[0] == (leg1_id, 1)
        assert legs[1] == (leg2_id, -1)

    def test_option_spread_legs_with_ratio_spread(self):
        """
        Test that legs() method correctly handles ratio spreads for option spreads.
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = new_generic_spread_id([(leg1_id, 2), (leg2_id, -3)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

        leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.01, underSymbol="SPY")

        leg_contract_details = [(leg1_details, 2), (leg2_details, -3)]

        # Create option spread with generic spread ID
        option_spread = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        legs = option_spread.legs()

        assert len(legs) == 2
        assert legs[0] == (leg1_id, 2)
        assert legs[1] == (leg2_id, -3)

    def test_option_spread_legs_with_butterfly_spread(self):
        """
        Test that legs() method correctly handles multi-leg spreads (butterfly) for
        option spreads.
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C405.SMART")
        leg3_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -2), (leg3_id, 1)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

        leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.01, underSymbol="SPY")

        leg3_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg3_details = IBContractDetails(contract=leg3_contract, minTick=0.01, underSymbol="SPY")

        leg_contract_details = [(leg1_details, 1), (leg2_details, -2), (leg3_details, 1)]

        # Create option spread with generic spread ID
        option_spread = parse_option_spread_instrument_id(spread_id, leg_contract_details)

        legs = option_spread.legs()

        assert len(legs) == 3
        assert legs[0] == (leg1_id, 1)
        assert legs[1] == (leg2_id, -2)
        assert legs[2] == (leg3_id, 1)

    def test_futures_spread_legs_with_generic_spread_id(self):
        """
        Test that legs() method correctly parses generic spread IDs for futures spreads.
        """
        leg1_id = InstrumentId.from_str("ESM4.XCME")
        leg2_id = InstrumentId.from_str("ESU4.XCME")
        spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.25, underSymbol="ES")

        leg2_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.25, underSymbol="ES")

        leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

        # Create futures spread with generic spread ID
        futures_spread = parse_futures_spread_instrument_id(spread_id, leg_contract_details)

        legs = futures_spread.legs()

        assert len(legs) == 2
        assert legs[0] == (leg1_id, 1)
        assert legs[1] == (leg2_id, -1)

    def test_futures_spread_legs_with_ratio_spread(self):
        """
        Test that legs() method correctly handles ratio spreads for futures spreads.
        """
        leg1_id = InstrumentId.from_str("ESM4.XCME")
        leg2_id = InstrumentId.from_str("ESU4.XCME")
        spread_id = new_generic_spread_id([(leg1_id, 2), (leg2_id, -3)])

        # Create mock contract details for legs
        leg1_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.25, underSymbol="ES")

        leg2_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.25, underSymbol="ES")

        leg_contract_details = [(leg1_details, 2), (leg2_details, -3)]

        # Create futures spread with generic spread ID
        futures_spread = parse_futures_spread_instrument_id(spread_id, leg_contract_details)

        legs = futures_spread.legs()

        assert len(legs) == 2
        assert legs[0] == (leg1_id, 2)
        assert legs[1] == (leg2_id, -3)

    def test_option_spread_legs_with_non_generic_spread_id(self):
        """
        Test that legs() method returns fallback for non-generic spread IDs.
        """
        # Create a non-generic spread ID
        non_generic_id = InstrumentId.from_str("SPY_SPREAD.SMART")

        # Create mock contract details
        leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

        leg_contract_details = [(leg1_details, 1)]

        # Create option spread with non-generic spread ID
        option_spread = parse_option_spread_instrument_id(non_generic_id, leg_contract_details)

        legs = option_spread.legs()

        # For non-generic spread IDs, should return [(self.id, 1)]
        assert len(legs) == 1
        assert legs[0] == (option_spread.id, 1)

    def test_futures_spread_legs_with_non_generic_spread_id(self):
        """
        Test that legs() method returns fallback for non-generic spread IDs.
        """
        # Create a non-generic spread ID
        non_generic_id = InstrumentId.from_str("ES_SPREAD.XCME")

        # Create mock contract details
        leg1_contract = IBContract(secType="FUT", symbol="ES", currency="USD", multiplier="50")
        leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.25, underSymbol="ES")

        leg_contract_details = [(leg1_details, 1)]

        # Create futures spread with non-generic spread ID
        futures_spread = parse_futures_spread_instrument_id(non_generic_id, leg_contract_details)

        legs = futures_spread.legs()

        # For non-generic spread IDs, should return [(self.id, 1)]
        assert len(legs) == 1
        assert legs[0] == (futures_spread.id, 1)
