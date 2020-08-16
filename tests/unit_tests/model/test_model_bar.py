# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.bar import BarSpecification, BarType, Bar
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class BarSpecificationTests(unittest.TestCase):

    def test_bar_spec_equality(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)
        bar_spec2 = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)
        bar_spec3 = BarSpecification(1, BarStructure.MINUTE, PriceType.ASK)

        # Act
        # Assert
        self.assertTrue(bar_spec1 == bar_spec1)
        self.assertTrue(bar_spec1 == bar_spec2)
        self.assertTrue(bar_spec1 != bar_spec3)

    def test_bar_spec_str_and_repr(self):
        # Arrange
        bar_spec = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)

        # Act
        # Assert
        self.assertEqual("1-MINUTE-BID", str(bar_spec))
        self.assertTrue(repr(bar_spec).startswith("<BarSpecification(1-MINUTE-BID) object at"))

    def test_can_parse_bar_spec_from_string(self):
        # Arrange
        bar_spec = BarSpecification(1, BarStructure.MINUTE, PriceType.MID)

        # Act
        result = BarSpecification.py_from_string(str(bar_spec))

        # Assert
        self.assertEqual(bar_spec, result)


class BarTypeTests(unittest.TestCase):

    def test_bar_type_equality(self):
        # Arrange
        symbol1 = Symbol("AUD/USD", Venue('FXCM'))
        symbol2 = Symbol("GBP/USD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)
        bar_type1 = BarType(symbol1, bar_spec)
        bar_type2 = BarType(symbol1, bar_spec)
        bar_type3 = BarType(symbol2, bar_spec)

        # Act
        # Assert
        self.assertTrue(bar_type1 == bar_type1)
        self.assertTrue(bar_type1 == bar_type2)
        self.assertTrue(bar_type1 != bar_type3)

    def test_bar_type_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUD/USD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)

        # Act
        # Assert
        self.assertEqual("AUD/USD.FXCM-1-MINUTE-BID", str(bar_type))
        self.assertTrue(repr(bar_type).startswith("<BarType(AUD/USD.FXCM-1-MINUTE-BID) object at"))


class BarTests(unittest.TestCase):

    def test_can_parse_bar_from_string(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        result = Bar.py_from_serializable_string(bar.to_serializable_string())

        # Assert
        self.assertEqual(bar, result)
