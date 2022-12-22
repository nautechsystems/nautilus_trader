# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetClassParser
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.enums import AssetTypeParser
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BarAggregationParser
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookActionParser
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import BookTypeParser
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import ContingencyTypeParser
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import CurrencyTypeParser
from nautilus_trader.model.enums import DepthType
from nautilus_trader.model.enums import DepthTypeParser
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentCloseTypeParser
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import InstrumentStatusParser
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import LiquiditySideParser
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OMSTypeParser
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OptionKindParser
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderStatusParser
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import OrderTypeParser
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import PositionSideParser
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import PriceTypeParser
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TimeInForceParser
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.enums import TradingStateParser
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TrailingOffsetTypeParser
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import TriggerTypeParser
from nautilus_trader.model.enums import VenueStatus
from nautilus_trader.model.enums import VenueStatusParser
from nautilus_trader.model.enums import account_type_from_str
from nautilus_trader.model.enums import account_type_to_str
from nautilus_trader.model.enums import aggregation_source_from_str
from nautilus_trader.model.enums import aggregation_source_to_str
from nautilus_trader.model.enums import aggressor_side_from_str
from nautilus_trader.model.enums import aggressor_side_to_str


class TestAccountType:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AccountType.CASH, "CASH"],
            [AccountType.MARGIN, "MARGIN"],
        ],
    )
    def test_account_type_to_str(self, enum, expected):
        # Arrange, Act
        result = account_type_to_str(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["CASH", AccountType.CASH],
            ["MARGIN", AccountType.MARGIN],
        ],
    )
    def test_account_type_from_str(self, string, expected):
        # Arrange, Act
        result = account_type_from_str(string)

        # Assert
        assert expected == result

    def test_instantiate_from_string(self):
        assert AccountType["CASH"] == AccountType.CASH


class TestAggregationSource:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AggregationSource.EXTERNAL, "EXTERNAL"],
            [AggregationSource.INTERNAL, "INTERNAL"],
        ],
    )
    def test_aggregation_source_to_str(self, enum, expected):
        # Arrange, Act
        result = aggregation_source_to_str(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["EXTERNAL", AggregationSource.EXTERNAL],
            ["INTERNAL", AggregationSource.INTERNAL],
        ],
    )
    def test_aggregation_source_from_str(self, string, expected):
        # Arrange, Act
        result = aggregation_source_from_str(string)

        # Assert
        assert expected == result


class TestAggressorSide:
    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AggressorSide.NONE, "NONE"],
            [AggressorSide.BUY, "BUY"],
            [AggressorSide.SELL, "SELL"],
        ],
    )
    def test_aggressor_side_to_str(self, enum, expected):
        # Arrange, Act
        result = aggressor_side_to_str(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", AggressorSide.NONE],
            ["BUY", AggressorSide.BUY],
            ["SELL", AggressorSide.SELL],
        ],
    )
    def test_aggressor_side_from_str(self, string, expected):
        # Arrange, Act
        result = aggressor_side_from_str(string)

        # Assert
        assert expected == result


class TestAssetClass:
    def test_asset_class_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            AssetClassParser.to_str_py(0)

        with pytest.raises(ValueError):
            AssetClassParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [AssetClass.FX, "FX"],
            [AssetClass.EQUITY, "EQUITY"],
            [AssetClass.COMMODITY, "COMMODITY"],
            [AssetClass.METAL, "METAL"],
            [AssetClass.ENERGY, "ENERGY"],
            [AssetClass.BOND, "BOND"],
            [AssetClass.INDEX, "INDEX"],
            [AssetClass.CRYPTO, "CRYPTO"],
            [AssetClass.BETTING, "BETTING"],
        ],
    )
    def test_asset_class_to_str(self, enum, expected):
        # Arrange, Act
        result = AssetClassParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["FX", AssetClass.FX],
            ["EQUITY", AssetClass.EQUITY],
            ["COMMODITY", AssetClass.COMMODITY],
            ["METAL", AssetClass.METAL],
            ["ENERGY", AssetClass.ENERGY],
            ["BOND", AssetClass.BOND],
            ["INDEX", AssetClass.INDEX],
            ["CRYPTO", AssetClass.CRYPTO],
            ["BETTING", AssetClass.BETTING],
        ],
    )
    def test_asset_class_from_str(self, string, expected):
        # Arrange, Act
        result = AssetClassParser.from_str_py(string)

        # Assert
        assert expected == result


class TestAssetType:
    def test_asset_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            AssetTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            AssetTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
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
        # Arrange, Act
        result = AssetTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
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
        # Arrange, Act
        result = AssetTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestBarAggregation:
    def test_bar_aggregation_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            BarAggregationParser.to_str_py(0)

        with pytest.raises(ValueError):
            BarAggregationParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [BarAggregation.TICK, "TICK"],
            [BarAggregation.TICK_IMBALANCE, "TICK_IMBALANCE"],
            [BarAggregation.TICK_RUNS, "TICK_RUNS"],
            [BarAggregation.VOLUME, "VOLUME"],
            [BarAggregation.VOLUME_IMBALANCE, "VOLUME_IMBALANCE"],
            [BarAggregation.VOLUME_RUNS, "VOLUME_RUNS"],
            [BarAggregation.VALUE, "VALUE"],
            [BarAggregation.VALUE_IMBALANCE, "VALUE_IMBALANCE"],
            [BarAggregation.VALUE_RUNS, "VALUE_RUNS"],
            [BarAggregation.MILLISECOND, "MILLISECOND"],
            [BarAggregation.SECOND, "SECOND"],
            [BarAggregation.MINUTE, "MINUTE"],
            [BarAggregation.HOUR, "HOUR"],
            [BarAggregation.DAY, "DAY"],
            [BarAggregation.WEEK, "WEEK"],
            [BarAggregation.MONTH, "MONTH"],
        ],
    )
    def test_bar_aggregation_to_str(self, enum, expected):
        # Arrange, Act
        result = BarAggregationParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["TICK", BarAggregation.TICK],
            ["TICK_IMBALANCE", BarAggregation.TICK_IMBALANCE],
            ["TICK_RUNS", BarAggregation.TICK_RUNS],
            ["VOLUME", BarAggregation.VOLUME],
            ["VOLUME_IMBALANCE", BarAggregation.VOLUME_IMBALANCE],
            ["VOLUME_RUNS", BarAggregation.VOLUME_RUNS],
            ["VALUE", BarAggregation.VALUE],
            ["VALUE_IMBALANCE", BarAggregation.VALUE_IMBALANCE],
            ["VALUE_RUNS", BarAggregation.VALUE_RUNS],
            ["MILLISECOND", BarAggregation.MILLISECOND],
            ["SECOND", BarAggregation.SECOND],
            ["MINUTE", BarAggregation.MINUTE],
            ["HOUR", BarAggregation.HOUR],
            ["DAY", BarAggregation.DAY],
            ["WEEK", BarAggregation.WEEK],
            ["MONTH", BarAggregation.MONTH],
        ],
    )
    def test_bar_aggregation_from_str(self, string, expected):
        # Arrange, Act
        result = BarAggregationParser.from_str_py(string)

        # Assert
        assert expected == result


class TestContingencyType:
    def test_contingency_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            ContingencyTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [ContingencyType.NONE, "NONE"],
            [ContingencyType.OCO, "OCO"],
            [ContingencyType.OTO, "OTO"],
            [ContingencyType.OUO, "OUO"],
        ],
    )
    def test_contingency_type_to_str(self, enum, expected):
        # Arrange, Act
        result = ContingencyTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", ContingencyType.NONE],
            ["OCO", ContingencyType.OCO],
            ["OTO", ContingencyType.OTO],
            ["OUO", ContingencyType.OUO],
        ],
    )
    def test_contingency_type_from_str(self, string, expected):
        # Arrange, Act
        result = ContingencyTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestCurrencyType:
    def test_currency_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            CurrencyTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            CurrencyTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [CurrencyType.CRYPTO, "CRYPTO"],
            [CurrencyType.FIAT, "FIAT"],
        ],
    )
    def test_currency_type_to_str(self, enum, expected):
        # Arrange, Act
        result = CurrencyTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["CRYPTO", CurrencyType.CRYPTO],
            ["FIAT", CurrencyType.FIAT],
        ],
    )
    def test_currency_type_from_str(self, string, expected):
        # Arrange, Act
        result = CurrencyTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestDepthType:
    def test_depth_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            DepthTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            DepthTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [DepthType.VOLUME, "VOLUME"],
            [DepthType.EXPOSURE, "EXPOSURE"],
        ],
    )
    def test_depth_type_to_str(self, enum, expected):
        # Arrange, Act
        result = DepthTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["VOLUME", DepthType.VOLUME],
            ["EXPOSURE", DepthType.EXPOSURE],
        ],
    )
    def test_depth_type_from_str(self, string, expected):
        # Arrange, Act
        result = DepthTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOptionKind:
    def test_option_kind_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            OptionKindParser.to_str_py(0)

        with pytest.raises(ValueError):
            OptionKindParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OptionKind.CALL, "CALL"],
            [OptionKind.PUT, "PUT"],
        ],
    )
    def test_option_kind_to_str(self, enum, expected):
        # Arrange, Act
        result = OptionKindParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["CALL", OptionKind.CALL],
            ["PUT", OptionKind.PUT],
        ],
    )
    def test_option_kind_from_str(self, string, expected):
        # Arrange, Act
        result = OptionKindParser.from_str_py(string)

        # Assert
        assert expected == result


class TestInstrumentCloseType:
    def test_instrument_close_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            InstrumentCloseTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            InstrumentCloseTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [InstrumentCloseType.END_OF_SESSION, "END_OF_SESSION"],
            [InstrumentCloseType.EXPIRED, "EXPIRED"],
        ],
    )
    def test_instrument_close_type_to_str(self, enum, expected):
        # Arrange, Act
        result = InstrumentCloseTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["END_OF_SESSION", InstrumentCloseType.END_OF_SESSION],
            ["EXPIRED", InstrumentCloseType.EXPIRED],
        ],
    )
    def test_instrument_close_type_from_str(self, string, expected):
        # Arrange, Act
        result = InstrumentCloseTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestInstrumentStatus:
    def test_instrument_status_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            InstrumentStatusParser.to_str_py(0)

        with pytest.raises(ValueError):
            InstrumentStatusParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [InstrumentStatus.CLOSED, "CLOSED"],
            [InstrumentStatus.PRE_OPEN, "PRE_OPEN"],
            [InstrumentStatus.OPEN, "OPEN"],
            [InstrumentStatus.PAUSE, "PAUSE"],
            [InstrumentStatus.PRE_CLOSE, "PRE_CLOSE"],
        ],
    )
    def test_instrument_status_to_str(self, enum, expected):
        # Arrange, Act
        result = InstrumentStatusParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["CLOSED", InstrumentStatus.CLOSED],
            ["PRE_OPEN", InstrumentStatus.PRE_OPEN],
            ["OPEN", InstrumentStatus.OPEN],
            ["PAUSE", InstrumentStatus.PAUSE],
            ["PRE_CLOSE", InstrumentStatus.PRE_CLOSE],
        ],
    )
    def test_instrument_status_from_str(self, string, expected):
        # Arrange, Act
        result = InstrumentStatusParser.from_str_py(string)

        # Assert
        assert expected == result


class TestLiquiditySide:
    def test_liquidity_side_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            LiquiditySideParser.to_str_py(9)

        with pytest.raises(ValueError):
            LiquiditySideParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [LiquiditySide.NONE, "NONE"],
            [LiquiditySide.MAKER, "MAKER"],
            [LiquiditySide.TAKER, "TAKER"],
        ],
    )
    def test_liquidity_side_to_str(self, enum, expected):
        # Arrange, Act
        result = LiquiditySideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", LiquiditySide.NONE],
            ["MAKER", LiquiditySide.MAKER],
            ["TAKER", LiquiditySide.TAKER],
        ],
    )
    def test_liquidity_side_from_str(self, string, expected):
        # Arrange, Act
        result = LiquiditySideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOMSType:
    def test_oms_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            OMSTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OMSType.NONE, "NONE"],
            [OMSType.NETTING, "NETTING"],
            [OMSType.HEDGING, "HEDGING"],
        ],
    )
    def test_oms_type_to_str(self, enum, expected):
        # Arrange, Act
        result = OMSTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", OMSType.NONE],
            ["NETTING", OMSType.NETTING],
            ["HEDGING", OMSType.HEDGING],
        ],
    )
    def test_oms_type_from_str(self, string, expected):
        # Arrange, Act
        result = OMSTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderSide:
    def test_order_side_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            OrderSideParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderSide.NONE, "NONE"],
            [OrderSide.BUY, "BUY"],
            [OrderSide.SELL, "SELL"],
        ],
    )
    def test_order_side_to_str(self, enum, expected):
        # Arrange, Act
        result = OrderSideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", OrderSide.NONE],
            ["BUY", OrderSide.BUY],
            ["SELL", OrderSide.SELL],
        ],
    )
    def test_order_side_from_str(self, string, expected):
        # Arrange, Act
        result = OrderSideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderStatus:
    def test_order_status_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            OrderStatusParser.to_str_py(0)

        with pytest.raises(ValueError):
            OrderStatusParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderStatus.INITIALIZED, "INITIALIZED"],
            [OrderStatus.DENIED, "DENIED"],
            [OrderStatus.SUBMITTED, "SUBMITTED"],
            [OrderStatus.ACCEPTED, "ACCEPTED"],
            [OrderStatus.REJECTED, "REJECTED"],
            [OrderStatus.CANCELED, "CANCELED"],
            [OrderStatus.EXPIRED, "EXPIRED"],
            [OrderStatus.TRIGGERED, "TRIGGERED"],
            [OrderStatus.PENDING_CANCEL, "PENDING_CANCEL"],
            [OrderStatus.PENDING_UPDATE, "PENDING_UPDATE"],
            [OrderStatus.PARTIALLY_FILLED, "PARTIALLY_FILLED"],
            [OrderStatus.FILLED, "FILLED"],
        ],
    )
    def test_order_status_to_str(self, enum, expected):
        # Arrange, Act
        result = OrderStatusParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["INITIALIZED", OrderStatus.INITIALIZED],
            ["DENIED", OrderStatus.DENIED],
            ["SUBMITTED", OrderStatus.SUBMITTED],
            ["ACCEPTED", OrderStatus.ACCEPTED],
            ["REJECTED", OrderStatus.REJECTED],
            ["CANCELED", OrderStatus.CANCELED],
            ["EXPIRED", OrderStatus.EXPIRED],
            ["TRIGGERED", OrderStatus.TRIGGERED],
            ["PENDING_CANCEL", OrderStatus.PENDING_CANCEL],
            ["PENDING_UPDATE", OrderStatus.PENDING_UPDATE],
            ["PARTIALLY_FILLED", OrderStatus.PARTIALLY_FILLED],
            ["FILLED", OrderStatus.FILLED],
        ],
    )
    def test_order_status_from_str(self, string, expected):
        # Arrange, Act
        result = OrderStatusParser.from_str_py(string)

        # Assert
        assert expected == result


class TestOrderType:
    def test_order_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            OrderTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            OrderTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [OrderType.MARKET, "MARKET"],
            [OrderType.LIMIT, "LIMIT"],
            [OrderType.STOP_MARKET, "STOP_MARKET"],
            [OrderType.STOP_LIMIT, "STOP_LIMIT"],
            [OrderType.MARKET_TO_LIMIT, "MARKET_TO_LIMIT"],
            [OrderType.MARKET_IF_TOUCHED, "MARKET_IF_TOUCHED"],
            [OrderType.LIMIT_IF_TOUCHED, "LIMIT_IF_TOUCHED"],
            [OrderType.TRAILING_STOP_MARKET, "TRAILING_STOP_MARKET"],
            [OrderType.TRAILING_STOP_LIMIT, "TRAILING_STOP_LIMIT"],
        ],
    )
    def test_order_type_to_str(self, enum, expected):
        # Arrange, Act
        result = OrderTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["MARKET", OrderType.MARKET],
            ["LIMIT", OrderType.LIMIT],
            ["STOP_MARKET", OrderType.STOP_MARKET],
            ["STOP_LIMIT", OrderType.STOP_LIMIT],
            ["MARKET_TO_LIMIT", OrderType.MARKET_TO_LIMIT],
            ["MARKET_IF_TOUCHED", OrderType.MARKET_IF_TOUCHED],
            ["LIMIT_IF_TOUCHED", OrderType.LIMIT_IF_TOUCHED],
            ["TRAILING_STOP_MARKET", OrderType.TRAILING_STOP_MARKET],
            ["TRAILING_STOP_LIMIT", OrderType.TRAILING_STOP_LIMIT],
        ],
    )
    def test_order_type_from_str(self, string, expected):
        # Arrange, Act
        result = OrderTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestBookLevel:
    def test_orderbook_level_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            BookTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            BookTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [BookType.L1_TBBO, "L1_TBBO"],
            [BookType.L2_MBP, "L2_MBP"],
            [BookType.L3_MBO, "L3_MBO"],
        ],
    )
    def test_orderbook_level_to_str(self, enum, expected):
        # Arrange, Act
        result = BookTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", None],
            ["L1_TBBO", BookType.L1_TBBO],
            ["L2_MBP", BookType.L2_MBP],
            ["L3_MBO", BookType.L3_MBO],
        ],
    )
    def test_orderbook_level_from_str(self, string, expected):
        # Arrange, Act
        if expected is None:
            return

        result = BookTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestDeltaType:
    def test_delta_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            BookActionParser.to_str_py(0)

        with pytest.raises(ValueError):
            BookActionParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [BookAction.ADD, "ADD"],
            [BookAction.UPDATE, "UPDATE"],
            [BookAction.DELETE, "DELETE"],
            [BookAction.CLEAR, "CLEAR"],
        ],
    )
    def test_delta_type_to_str(self, enum, expected):
        # Arrange, Act
        result = BookActionParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["", None],
            ["ADD", BookAction.ADD],
            ["UPDATE", BookAction.UPDATE],
            ["DELETE", BookAction.DELETE],
            ["CLEAR", BookAction.CLEAR],
        ],
    )
    def test_delta_type_from_str(self, string, expected):
        # Arrange, Act
        if expected is None:
            return

        result = BookActionParser.from_str_py(string)

        # Assert
        assert expected == result


class TestPositionSide:
    def test_position_side_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PositionSideParser.to_str_py(-1)

        with pytest.raises(ValueError):
            PositionSideParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [PositionSide.NONE, "NONE"],
            [PositionSide.FLAT, "FLAT"],
            [PositionSide.LONG, "LONG"],
            [PositionSide.SHORT, "SHORT"],
        ],
    )
    def test_position_side_to_str(self, enum, expected):
        # Arrange, Act
        result = PositionSideParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", PositionSide.NONE],
            ["FLAT", PositionSide.FLAT],
            ["LONG", PositionSide.LONG],
            ["SHORT", PositionSide.SHORT],
        ],
    )
    def test_position_side_from_str(self, string, expected):
        # Arrange, Act
        result = PositionSideParser.from_str_py(string)

        # Assert
        assert expected == result


class TestPriceType:
    def test_price_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PriceTypeParser.to_str_py(0)

        with pytest.raises(ValueError):
            PriceTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [PriceType.BID, "BID"],
            [PriceType.ASK, "ASK"],
            [PriceType.MID, "MID"],
            [PriceType.LAST, "LAST"],
        ],
    )
    def test_price_type_to_str(self, enum, expected):
        # Arrange, Act
        result = PriceTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["ASK", PriceType.ASK],
            ["MID", PriceType.MID],
            ["LAST", PriceType.LAST],
        ],
    )
    def test_price_type_from_str(self, string, expected):
        # Arrange, Act
        result = PriceTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestTimeInForce:
    def test_time_in_force_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            TimeInForceParser.to_str_py(0)

        with pytest.raises(ValueError):
            TimeInForceParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [TimeInForce.GTC, "GTC"],
            [TimeInForce.IOC, "IOC"],
            [TimeInForce.FOK, "FOK"],
            [TimeInForce.GTD, "GTD"],
            [TimeInForce.DAY, "DAY"],
            [TimeInForce.AT_THE_OPEN, "AT_THE_OPEN"],
            [TimeInForce.AT_THE_CLOSE, "AT_THE_CLOSE"],
        ],
    )
    def test_time_in_force_to_str(self, enum, expected):
        # Arrange, Act
        result = TimeInForceParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["GTC", TimeInForce.GTC],
            ["IOC", TimeInForce.IOC],
            ["FOK", TimeInForce.FOK],
            ["GTD", TimeInForce.GTD],
            ["DAY", TimeInForce.DAY],
            ["AT_THE_OPEN", TimeInForce.AT_THE_OPEN],
            ["AT_THE_CLOSE", TimeInForce.AT_THE_CLOSE],
        ],
    )
    def test_time_in_force_from_str(self, string, expected):
        # Arrange, Act
        result = TimeInForceParser.from_str_py(string)

        # Assert
        assert expected == result


class TestTradingState:
    def test_trading_state_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            TradingStateParser.to_str_py(0)

        with pytest.raises(ValueError):
            TradingStateParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [TradingState.ACTIVE, "ACTIVE"],
            [TradingState.HALTED, "HALTED"],
            [TradingState.REDUCING, "REDUCING"],
        ],
    )
    def test_trading_state_to_str(self, enum, expected):
        # Arrange, Act
        result = TradingStateParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["ACTIVE", TradingState.ACTIVE],
            ["HALTED", TradingState.HALTED],
            ["REDUCING", TradingState.REDUCING],
        ],
    )
    def test_trading_state_from_str(self, string, expected):
        # Arrange, Act
        result = TradingStateParser.from_str_py(string)

        # Assert
        assert expected == result


class TestTrailingOffsetType:
    def test_trading_state_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            TrailingOffsetTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [TrailingOffsetType.NONE, "NONE"],
            [TrailingOffsetType.DEFAULT, "DEFAULT"],
            [TrailingOffsetType.PRICE, "PRICE"],
            [TrailingOffsetType.BASIS_POINTS, "BASIS_POINTS"],
            [TrailingOffsetType.TICKS, "TICKS"],
            [TrailingOffsetType.PRICE_TIER, "PRICE_TIER"],
        ],
    )
    def test_trailing_offset_type_to_str(self, enum, expected):
        # Arrange, Act
        result = TrailingOffsetTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", TrailingOffsetType.NONE],
            ["DEFAULT", TrailingOffsetType.DEFAULT],
            ["PRICE", TrailingOffsetType.PRICE],
            ["BASIS_POINTS", TrailingOffsetType.BASIS_POINTS],
            ["TICKS", TrailingOffsetType.TICKS],
            ["PRICE_TIER", TrailingOffsetType.PRICE_TIER],
        ],
    )
    def test_trailing_offset_type_from_str(self, string, expected):
        # Arrange, Act
        result = TrailingOffsetTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestTriggerType:
    def test_trigger_type_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            TriggerTypeParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [TriggerType.NONE, "NONE"],
            [TriggerType.DEFAULT, "DEFAULT"],
            [TriggerType.LAST, "LAST"],
            [TriggerType.BID_ASK, "BID_ASK"],
            [TriggerType.DOUBLE_LAST, "DOUBLE_LAST"],
            [TriggerType.DOUBLE_BID_ASK, "DOUBLE_BID_ASK"],
            [TriggerType.LAST_OR_BID_ASK, "LAST_OR_BID_ASK"],
            [TriggerType.MID_POINT, "MID_POINT"],
            [TriggerType.MARK, "MARK"],
            [TriggerType.INDEX, "INDEX"],
        ],
    )
    def test_trigger_type_to_str(self, enum, expected):
        # Arrange, Act
        result = TriggerTypeParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["NONE", TriggerType.NONE],
            ["DEFAULT", TriggerType.DEFAULT],
            ["LAST", TriggerType.LAST],
            ["BID_ASK", TriggerType.BID_ASK],
            ["DOUBLE_LAST", TriggerType.DOUBLE_LAST],
            ["DOUBLE_BID_ASK", TriggerType.DOUBLE_BID_ASK],
            ["LAST_OR_BID_ASK", TriggerType.LAST_OR_BID_ASK],
            ["MID_POINT", TriggerType.MID_POINT],
            ["MARK", TriggerType.MARK],
            ["INDEX", TriggerType.INDEX],
        ],
    )
    def test_trigger_type_from_str(self, string, expected):
        # Arrange, Act
        result = TriggerTypeParser.from_str_py(string)

        # Assert
        assert expected == result


class TestVenueStatus:
    def test_venue_status_parser_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            VenueStatusParser.to_str_py(0)

        with pytest.raises(ValueError):
            VenueStatusParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum, expected",
        [
            [VenueStatus.CLOSED, "CLOSED"],
            [VenueStatus.PRE_OPEN, "PRE_OPEN"],
            [VenueStatus.OPEN, "OPEN"],
            [VenueStatus.PAUSE, "PAUSE"],
            [VenueStatus.PRE_CLOSE, "PRE_CLOSE"],
        ],
    )
    def test_venue_status_to_str(self, enum, expected):
        # Arrange, Act
        result = VenueStatusParser.to_str_py(enum)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["CLOSED", VenueStatus.CLOSED],
            ["PRE_OPEN", VenueStatus.PRE_OPEN],
            ["OPEN", VenueStatus.OPEN],
            ["PAUSE", VenueStatus.PAUSE],
            ["PRE_CLOSE", VenueStatus.PRE_CLOSE],
        ],
    )
    def test_venue_status_from_str(self, string, expected):
        # Arrange, Act
        result = VenueStatusParser.from_str_py(string)

        # Assert
        assert expected == result
