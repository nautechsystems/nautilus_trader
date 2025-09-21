#!/usr/bin/env python3
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
"""
Comprehensive tests for Interactive Brokers instrument parsing, especially spread
instruments.
"""


import pytest

# fmt: off
# ruff: noqa: I001
from nautilus_trader.adapters.interactive_brokers.common import IBContract, IBContractDetails
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    parse_option_spread,
    parse_spread_instrument_id,
)
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import OptionSpread
from nautilus_trader.model.objects import Currency, Price, Quantity
# fmt: on


class TestSpreadInstrumentParsing:
    """
    Test cases for parsing spread instruments from instrument IDs.
    """

    def test_parse_spread_instrument_id_basic_spread(self):
        """
        Test parsing basic 1x1 spread instrument ID.
        """
        # Create spread instrument ID
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = InstrumentId.new_spread([(leg1_id, 1), (leg2_id, -1)])

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
        instrument = parse_spread_instrument_id(spread_id, leg_contract_details)

        # Verify the result
        assert isinstance(instrument, OptionSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "SPY"
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(100)
        assert instrument.lot_size == Quantity.from_int(100)  # Should equal multiplier
        assert instrument.price_increment == Price.from_str("0.01")

    def test_parse_spread_instrument_id_ratio_spread(self):
        """
        Test parsing ratio spread instrument ID.
        """
        leg1_id = InstrumentId.from_str("E4DN5 P6350.XCME")
        leg2_id = InstrumentId.from_str("E4DN5 P6355.XCME")
        spread_id = InstrumentId.new_spread([(leg1_id, 1), (leg2_id, -2)])

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

        instrument = parse_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.id == spread_id
        assert instrument.strategy_type == "SPREAD"
        assert instrument.underlying == "ES"  # ES futures options
        assert instrument.quote_currency == Currency.from_str("USD")
        assert instrument.multiplier == Quantity.from_int(50)
        assert instrument.lot_size == Quantity.from_int(50)  # Should equal multiplier
        assert instrument.price_increment == Price.from_str("0.05")

    def test_parse_spread_instrument_id_butterfly(self):
        """
        Test parsing butterfly spread (3 legs).
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C405.SMART")
        leg3_id = InstrumentId.from_str("SPY C410.SMART")
        spread_id = InstrumentId.new_spread([(leg1_id, 1), (leg2_id, -2), (leg3_id, 1)])

        # Create mock contract details for legs
        leg_contract_details = []
        for leg_id, ratio in [(leg1_id, 1), (leg2_id, -2), (leg3_id, 1)]:
            contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
            details = IBContractDetails(contract=contract, minTick=0.01, underSymbol="SPY")
            leg_contract_details.append((details, ratio))

        instrument = parse_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def test_parse_spread_instrument_id_iron_condor(self):
        """
        Test parsing iron condor spread (4 legs).
        """
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C405.SMART")
        leg3_id = InstrumentId.from_str("SPY P395.SMART")
        leg4_id = InstrumentId.from_str("SPY P390.SMART")
        spread_id = InstrumentId.new_spread(
            [
                (leg1_id, 1),
                (leg2_id, -1),
                (leg3_id, 1),
                (leg4_id, -1),
            ],
        )

        # Create mock contract details for legs
        leg_contract_details = []
        for leg_id, ratio in [(leg1_id, 1), (leg2_id, -1), (leg3_id, 1), (leg4_id, -1)]:
            contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
            details = IBContractDetails(contract=contract, minTick=0.01, underSymbol="SPY")
            leg_contract_details.append((details, ratio))

        instrument = parse_spread_instrument_id(spread_id, leg_contract_details)

        assert isinstance(instrument, OptionSpread)
        assert instrument.strategy_type == "SPREAD"

    def test_parse_spread_instrument_id_invalid(self):
        """
        Test parsing invalid spread instrument ID.
        """
        # Create invalid spread ID (no legs)
        invalid_id = InstrumentId.from_str("INVALID.SMART")

        with pytest.raises(ValueError, match="leg_contract_details must be provided"):
            parse_spread_instrument_id(invalid_id, [])


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

        instrument_id = InstrumentId.from_str("(1)SPY C400_((1))SPY C410.SMART")

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
    """
    Test that spread instrument parsing integration works.
    """
    from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
        parse_spread_instrument_id,
    )

    # Test basic functionality
    leg1_id = InstrumentId.from_str("SPY C400.SMART")
    leg2_id = InstrumentId.from_str("SPY C410.SMART")
    spread_id = InstrumentId.new_spread([(leg1_id, 1), (leg2_id, -1)])

    # Create mock contract details
    leg1_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
    leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.01, underSymbol="SPY")

    leg2_contract = IBContract(secType="OPT", symbol="SPY", currency="USD", multiplier="100")
    leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.01, underSymbol="SPY")

    leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]

    instrument = parse_spread_instrument_id(spread_id, leg_contract_details)

    assert isinstance(instrument, OptionSpread)
    assert instrument.id == spread_id

    print("Spread instrument parsing working correctly")
