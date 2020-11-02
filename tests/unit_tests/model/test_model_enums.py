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

from parameterized import parameterized

from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.asset_class import AssetClass
from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.c_enums.asset_type import AssetType
from nautilus_trader.model.c_enums.asset_type import AssetTypeParser
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.c_enums.currency_type import CurrencyType
from nautilus_trader.model.c_enums.currency_type import CurrencyTypeParser
# from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide
# from nautilus_trader.model.c_enums.maker import Maker
# from nautilus_trader.model.c_enums.oms_type import OMSType
# from nautilus_trader.model.c_enums.order_side import OrderSide
# from nautilus_trader.model.c_enums.order_state import OrderState
# from nautilus_trader.model.c_enums.order_type import OrderType
# from nautilus_trader.model.c_enums.position_side import PositionSide
# from nautilus_trader.model.c_enums.price_type import PriceType
# from nautilus_trader.model.c_enums.time_in_force import TimeInForce


class AccountTypeTests(unittest.TestCase):

    @parameterized.expand([
        [AccountType.UNDEFINED, "UNDEFINED"],
        [AccountType.SIMULATED, "SIMULATED"],
        [AccountType.DEMO, "DEMO"],
        [AccountType.REAL, "REAL"],
    ])
    def test_account_type_to_string(self, enum, expected):
        # Arrange
        # Act
        result = AccountTypeParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", AccountType.UNDEFINED],
        ["UNDEFINED", AccountType.UNDEFINED],
        ["SIMULATED", AccountType.SIMULATED],
        ["DEMO", AccountType.DEMO],
        ["REAL", AccountType.REAL],
    ])
    def test_account_type_from_string(self, string, expected):
        # Arrange
        # Act
        result = AccountTypeParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)


class AssetClassTests(unittest.TestCase):

    @parameterized.expand([
        [AssetClass.UNDEFINED, "UNDEFINED"],
        [AssetClass.CRYPTO, "CRYPTO"],
        [AssetClass.FX, "FX"],
        [AssetClass.EQUITY, "EQUITY"],
        [AssetClass.COMMODITY, "COMMODITY"],
        [AssetClass.BOND, "BOND"],
    ])
    def test_asset_class_to_string(self, enum, expected):
        # Arrange
        # Act
        result = AssetClassParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", AssetClass.UNDEFINED],
        ["UNDEFINED", AssetClass.UNDEFINED],
        ["CRYPTO", AssetClass.CRYPTO],
        ["FX", AssetClass.FX],
        ["EQUITY", AssetClass.EQUITY],
        ["COMMODITY", AssetClass.COMMODITY],
        ["BOND", AssetClass.BOND],
    ])
    def test_asset_class_from_string(self, string, expected):
        # Arrange
        # Act
        result = AssetClassParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)


class AssetTypeTests(unittest.TestCase):

    @parameterized.expand([
        [AssetType.UNDEFINED, "UNDEFINED"],
        [AssetType.SPOT, "SPOT"],
        [AssetType.SWAP, "SWAP"],
        [AssetType.FUTURE, "FUTURE"],
        [AssetType.FORWARD, "FORWARD"],
        [AssetType.CFD, "CFD"],
        [AssetType.OPTION, "OPTION"],
    ])
    def test_asset_type_to_string(self, enum, expected):
        # Arrange
        # Act
        result = AssetTypeParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", AssetType.UNDEFINED],
        ["UNDEFINED", AssetType.UNDEFINED],
        ["SPOT", AssetType.SPOT],
        ["SWAP", AssetType.SWAP],
        ["FUTURE", AssetType.FUTURE],
        ["FORWARD", AssetType.FORWARD],
        ["CFD", AssetType.CFD],
        ["OPTION", AssetType.OPTION],
    ])
    def test_asset_type_from_string(self, string, expected):
        # Arrange
        # Act
        result = AssetTypeParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)


class BarAggregationTests(unittest.TestCase):

    @parameterized.expand([
        [BarAggregation.UNDEFINED, "UNDEFINED"],
        [BarAggregation.TICK, "TICK"],
        [BarAggregation.TICK_IMBALANCE, "TICK_IMBALANCE"],
        [BarAggregation.VOLUME, "VOLUME"],
        [BarAggregation.VOLUME_IMBALANCE, "VOLUME_IMBALANCE"],
        [BarAggregation.DOLLAR, "DOLLAR"],
        [BarAggregation.DOLLAR_IMBALANCE, "DOLLAR_IMBALANCE"],
        [BarAggregation.SECOND, "SECOND"],
        [BarAggregation.MINUTE, "MINUTE"],
        [BarAggregation.HOUR, "HOUR"],
        [BarAggregation.DAY, "DAY"],
    ])
    def test_bar_aggregation_to_string(self, enum, expected):
        # Arrange
        # Act
        result = BarAggregationParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", BarAggregation.UNDEFINED],
        ["UNDEFINED", BarAggregation.UNDEFINED],
        ["TICK", BarAggregation.TICK],
        ["TICK_IMBALANCE", BarAggregation.TICK_IMBALANCE],
        ["VOLUME", BarAggregation.VOLUME],
        ["VOLUME_IMBALANCE", BarAggregation.VOLUME_IMBALANCE],
        ["DOLLAR", BarAggregation.DOLLAR],
        ["DOLLAR_IMBALANCE", BarAggregation.DOLLAR_IMBALANCE],
        ["SECOND", BarAggregation.SECOND],
        ["MINUTE", BarAggregation.MINUTE],
        ["HOUR", BarAggregation.HOUR],
        ["DAY", BarAggregation.DAY],
    ])
    def test_bar_aggregation_from_string(self, string, expected):
        # Arrange
        # Act
        result = BarAggregationParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)


class CurrencyTypeTests(unittest.TestCase):

    @parameterized.expand([
        [CurrencyType.UNDEFINED, "UNDEFINED"],
        [CurrencyType.CRYPTO, "CRYPTO"],
        [CurrencyType.FIAT, "FIAT"],
    ])
    def test_currency_type_to_string(self, enum, expected):
        # Arrange
        # Act
        result = CurrencyTypeParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", CurrencyType.UNDEFINED],
        ["UNDEFINED", CurrencyType.UNDEFINED],
        ["CRYPTO", CurrencyType.CRYPTO],
        ["FIAT", CurrencyType.FIAT],
    ])
    def test_currency_type_from_string(self, string, expected):
        # Arrange
        # Act
        result = CurrencyTypeParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)
