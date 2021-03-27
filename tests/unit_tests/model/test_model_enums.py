# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.model.c_enums.aggressor_side import AggressorSide
from nautilus_trader.model.c_enums.aggressor_side import AggressorSideParser
from nautilus_trader.model.c_enums.asset_class import AssetClass
from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.c_enums.asset_type import AssetType
from nautilus_trader.model.c_enums.asset_type import AssetTypeParser
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.c_enums.currency_type import CurrencyType
from nautilus_trader.model.c_enums.currency_type import CurrencyTypeParser
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySideParser
from nautilus_trader.model.c_enums.oms_type import OMSType
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.c_enums.order_state import OrderState
from nautilus_trader.model.c_enums.order_state import OrderStateParser
from nautilus_trader.model.c_enums.order_type import OrderType
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevelParser
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationTypeParser
from nautilus_trader.model.c_enums.position_side import PositionSide
from nautilus_trader.model.c_enums.position_side import PositionSideParser
from nautilus_trader.model.c_enums.price_type import PriceType
from nautilus_trader.model.c_enums.price_type import PriceTypeParser
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.c_enums.time_in_force import TimeInForceParser


class TestAggressorSide:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AggressorSide.UNDEFINED, "UNDEFINED"],
            [AggressorSide.BUY, "BUY"],
            [AggressorSide.SELL, "SELL"],
        ],
    )
    def test_aggressor_side_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderSideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", AggressorSide.UNDEFINED],
            ["UNDEFINED", AggressorSide.UNDEFINED],
            ["BUY", AggressorSide.BUY],
            ["SELL", AggressorSide.SELL],
        ],
    )
    def test_order_side_from_str(self, string, expected):
        # Arrange
        # Act
        result = AggressorSideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestAssetClass:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AssetClass.UNDEFINED, "UNDEFINED"],
            [AssetClass.FX, "FX"],
            [AssetClass.STOCK, "STOCK"],
            [AssetClass.COMMODITY, "COMMODITY"],
            [AssetClass.BOND, "BOND"],
            [AssetClass.INDEX, "INDEX"],
            [AssetClass.CRYPTO, "CRYPTO"],
            [AssetClass.BETTING, "BETTING"],
        ],
    )
    def test_asset_class_to_str(self, enum, expected):
        # Arrange
        # Act
        result = AssetClassParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", AssetClass.UNDEFINED],
            ["UNDEFINED", AssetClass.UNDEFINED],
            ["FX", AssetClass.FX],
            ["STOCK", AssetClass.STOCK],
            ["COMMODITY", AssetClass.COMMODITY],
            ["BOND", AssetClass.BOND],
            ["INDEX", AssetClass.INDEX],
            ["CRYPTO", AssetClass.CRYPTO],
            ["BETTING", AssetClass.BETTING],
        ],
    )
    def test_asset_class_from_str(self, string, expected):
        # Arrange
        # Act
        result = AssetClassParser.from_str_py(string)

        # Assert
        assert expected == result


class TestAssetType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AssetType.UNDEFINED, "UNDEFINED"],
            [AssetType.SPOT, "SPOT"],
            [AssetType.SWAP, "SWAP"],
            [AssetType.FUTURE, "FUTURE"],
            [AssetType.FORWARD, "FORWARD"],
            [AssetType.CFD, "CFD"],
            [AssetType.OPTION, "OPTION"],
            [AssetType.WARRANT, "WARRANT"],
        ],
    )
    def test_asset_type_to_str(self, enum, expected):
        # Arrange
        # Act
        result = AssetTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", AssetType.UNDEFINED],
            ["UNDEFINED", AssetType.UNDEFINED],
            ["SPOT", AssetType.SPOT],
            ["SWAP", AssetType.SWAP],
            ["FUTURE", AssetType.FUTURE],
            ["FORWARD", AssetType.FORWARD],
            ["CFD", AssetType.CFD],
            ["OPTION", AssetType.OPTION],
            ["WARRANT", AssetType.WARRANT],
        ],
    )
    def test_asset_type_from_str(self, string, expected):
        # Arrange
        # Act
        result = AssetTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestBarAggregation:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [BarAggregation.UNDEFINED, "UNDEFINED"],
            [BarAggregation.TICK, "TICK"],
            [BarAggregation.TICK_IMBALANCE, "TICK_IMBALANCE"],
            [BarAggregation.TICK_RUNS, "TICK_RUNS"],
            [BarAggregation.VOLUME, "VOLUME"],
            [BarAggregation.VOLUME_IMBALANCE, "VOLUME_IMBALANCE"],
            [BarAggregation.VOLUME_RUNS, "VOLUME_RUNS"],
            [BarAggregation.VALUE, "VALUE"],
            [BarAggregation.VALUE_IMBALANCE, "VALUE_IMBALANCE"],
            [BarAggregation.VALUE_RUNS, "VALUE_RUNS"],
            [BarAggregation.SECOND, "SECOND"],
            [BarAggregation.MINUTE, "MINUTE"],
            [BarAggregation.HOUR, "HOUR"],
            [BarAggregation.DAY, "DAY"],
        ],
    )
    def test_bar_aggregation_to_str(self, enum, expected):
        # Arrange
        # Act
        result = BarAggregationParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", BarAggregation.UNDEFINED],
            ["UNDEFINED", BarAggregation.UNDEFINED],
            ["TICK", BarAggregation.TICK],
            ["TICK_IMBALANCE", BarAggregation.TICK_IMBALANCE],
            ["TICK_RUNS", BarAggregation.TICK_RUNS],
            ["VOLUME", BarAggregation.VOLUME],
            ["VOLUME_IMBALANCE", BarAggregation.VOLUME_IMBALANCE],
            ["VOLUME_RUNS", BarAggregation.VOLUME_RUNS],
            ["VALUE", BarAggregation.VALUE],
            ["VALUE_IMBALANCE", BarAggregation.VALUE_IMBALANCE],
            ["VALUE_RUNS", BarAggregation.VALUE_RUNS],
            ["SECOND", BarAggregation.SECOND],
            ["MINUTE", BarAggregation.MINUTE],
            ["HOUR", BarAggregation.HOUR],
            ["DAY", BarAggregation.DAY],
        ],
    )
    def test_bar_aggregation_from_str(self, string, expected):
        # Arrange
        # Act
        result = BarAggregationParser.from_str_py(string)

        # Assert
        assert expected == result


class TestCurrencyType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [CurrencyType.UNDEFINED, "UNDEFINED"],
            [CurrencyType.CRYPTO, "CRYPTO"],
            [CurrencyType.FIAT, "FIAT"],
        ],
    )
    def test_currency_type_to_str(self, enum, expected):
        # Arrange
        # Act
        result = CurrencyTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", CurrencyType.UNDEFINED],
            ["UNDEFINED", CurrencyType.UNDEFINED],
            ["CRYPTO", CurrencyType.CRYPTO],
            ["FIAT", CurrencyType.FIAT],
        ],
    )
    def test_currency_type_from_str(self, string, expected):
        # Arrange
        # Act
        result = CurrencyTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestLiquiditySide:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [LiquiditySide.NONE, "NONE"],
            [LiquiditySide.MAKER, "MAKER"],
            [LiquiditySide.TAKER, "TAKER"],
        ],
    )
    def test_liquidity_side_to_str(self, enum, expected):
        # Arrange
        # Act
        result = LiquiditySideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", LiquiditySide.NONE],
            ["NONE", LiquiditySide.NONE],
            ["MAKER", LiquiditySide.MAKER],
            ["TAKER", LiquiditySide.TAKER],
        ],
    )
    def test_liquidity_side_from_str(self, string, expected):
        # Arrange
        # Act
        result = LiquiditySideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOMSType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OMSType.UNDEFINED, "UNDEFINED"],
            [OMSType.NETTING, "NETTING"],
            [OMSType.HEDGING, "HEDGING"],
        ],
    )
    def test_oms_type_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OMSTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", OMSType.UNDEFINED],
            ["UNDEFINED", OMSType.UNDEFINED],
            ["NETTING", OMSType.NETTING],
            ["HEDGING", OMSType.HEDGING],
        ],
    )
    def test_oms_type_from_str(self, string, expected):
        # Arrange
        # Act
        result = OMSTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderSide:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderSide.UNDEFINED, "UNDEFINED"],
            [OrderSide.BUY, "BUY"],
            [OrderSide.SELL, "SELL"],
        ],
    )
    def test_order_side_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderSideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", OrderSide.UNDEFINED],
            ["UNDEFINED", OrderSide.UNDEFINED],
            ["BUY", OrderSide.BUY],
            ["SELL", OrderSide.SELL],
        ],
    )
    def test_order_side_from_str(self, string, expected):
        # Arrange
        # Act
        result = OrderSideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderState:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderState.UNDEFINED, "UNDEFINED"],
            [OrderState.INITIALIZED, "INITIALIZED"],
            [OrderState.INVALID, "INVALID"],
            [OrderState.DENIED, "DENIED"],
            [OrderState.SUBMITTED, "SUBMITTED"],
            [OrderState.ACCEPTED, "ACCEPTED"],
            [OrderState.REJECTED, "REJECTED"],
            [OrderState.CANCELLED, "CANCELLED"],
            [OrderState.EXPIRED, "EXPIRED"],
            [OrderState.TRIGGERED, "TRIGGERED"],
            [OrderState.PARTIALLY_FILLED, "PARTIALLY_FILLED"],
            [OrderState.FILLED, "FILLED"],
        ],
    )
    def test_order_state_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderStateParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", OrderState.UNDEFINED],
            ["UNDEFINED", OrderState.UNDEFINED],
            ["INITIALIZED", OrderState.INITIALIZED],
            ["INVALID", OrderState.INVALID],
            ["DENIED", OrderState.DENIED],
            ["SUBMITTED", OrderState.SUBMITTED],
            ["ACCEPTED", OrderState.ACCEPTED],
            ["REJECTED", OrderState.REJECTED],
            ["CANCELLED", OrderState.CANCELLED],
            ["EXPIRED", OrderState.EXPIRED],
            ["TRIGGERED", OrderState.TRIGGERED],
            ["PARTIALLY_FILLED", OrderState.PARTIALLY_FILLED],
            ["FILLED", OrderState.FILLED],
        ],
    )
    def test_order_state_from_str(self, string, expected):
        # Arrange
        # Act
        result = OrderStateParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderType.UNDEFINED, "UNDEFINED"],
            [OrderType.MARKET, "MARKET"],
            [OrderType.LIMIT, "LIMIT"],
            [OrderType.STOP_MARKET, "STOP_MARKET"],
            [OrderType.STOP_LIMIT, "STOP_LIMIT"],
        ],
    )
    def test_order_type_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", OrderType.UNDEFINED],
            ["UNDEFINED", OrderType.UNDEFINED],
            ["MARKET", OrderType.MARKET],
            ["LIMIT", OrderType.LIMIT],
            ["STOP_MARKET", OrderType.STOP_MARKET],
            ["STOP_LIMIT", OrderType.STOP_LIMIT],
        ],
    )
    def test_order_type_from_str(self, string, expected):
        # Arrange
        # Act
        result = OrderTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderBookLevel:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderBookLevel.L1, "L1"],
            [OrderBookLevel.L2, "L2"],
            [OrderBookLevel.L3, "L3"],
        ],
    )
    def test_orderbook_level_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderBookLevelParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", None],
            ["L1", OrderBookLevel.L1],
            ["L2", OrderBookLevel.L2],
            ["L3", OrderBookLevel.L3],
        ],
    )
    def test_orderbook_level_from_str(self, string, expected):
        # Arrange
        # Act
        if expected is None:
            return

        result = OrderBookLevelParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderBookOperationType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderBookOperationType.ADD, "ADD"],
            [OrderBookOperationType.UPDATE, "UPDATE"],
            [OrderBookOperationType.DELETE, "DELETE"],
        ],
    )
    def test_orderbook_op_to_str(self, enum, expected):
        # Arrange
        # Act
        result = OrderBookOperationTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", None],
            ["ADD", OrderBookOperationType.ADD],
            ["UPDATE", OrderBookOperationType.UPDATE],
            ["DELETE", OrderBookOperationType.DELETE],
        ],
    )
    def test_orderbook_op_from_str(self, string, expected):
        # Arrange
        # Act
        if expected is None:
            return

        result = OrderBookOperationTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestPositionSide:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [PositionSide.UNDEFINED, "UNDEFINED"],
            [PositionSide.FLAT, "FLAT"],
            [PositionSide.LONG, "LONG"],
            [PositionSide.SHORT, "SHORT"],
        ],
    )
    def test_position_side_to_str(self, enum, expected):
        # Arrange
        # Act
        result = PositionSideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", PositionSide.UNDEFINED],
            ["UNDEFINED", PositionSide.UNDEFINED],
            ["FLAT", PositionSide.FLAT],
            ["LONG", PositionSide.LONG],
            ["SHORT", PositionSide.SHORT],
        ],
    )
    def test_position_side_from_str(self, string, expected):
        # Arrange
        # Act
        result = PositionSideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestPriceType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [PriceType.UNDEFINED, "UNDEFINED"],
            [PriceType.BID, "BID"],
            [PriceType.ASK, "ASK"],
            [PriceType.MID, "MID"],
            [PriceType.LAST, "LAST"],
        ],
    )
    def test_price_type_to_str(self, enum, expected):
        # Arrange
        # Act
        result = PriceTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", PriceType.UNDEFINED],
            ["UNDEFINED", PriceType.UNDEFINED],
            ["ASK", PriceType.ASK],
            ["MID", PriceType.MID],
            ["LAST", PriceType.LAST],
        ],
    )
    def test_price_type_from_str(self, string, expected):
        # Arrange
        # Act
        result = PriceTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestTimeInForce:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [TimeInForce.UNDEFINED, "UNDEFINED"],
            [TimeInForce.DAY, "DAY"],
            [TimeInForce.GTC, "GTC"],
            [TimeInForce.IOC, "IOC"],
            [TimeInForce.FOK, "FOK"],
            [TimeInForce.GTD, "GTD"],
        ],
    )
    def test_time_in_force_to_str(self, enum, expected):
        # Arrange
        # Act
        result = TimeInForceParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", TimeInForce.UNDEFINED],
            ["UNDEFINED", TimeInForce.UNDEFINED],
            ["DAY", TimeInForce.DAY],
            ["GTC", TimeInForce.GTC],
            ["IOC", TimeInForce.IOC],
            ["FOK", TimeInForce.FOK],
            ["GTD", TimeInForce.GTD],
        ],
    )
    def test_time_in_force_from_str(self, string, expected):
        # Arrange
        # Act
        result = TimeInForceParser.from_str_py(string)

        # Assert
        assert expected == result
