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

import pytest

from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import account_type_from_str
from nautilus_trader.model.enums import account_type_to_str
from nautilus_trader.model.enums import aggregation_source_from_str
from nautilus_trader.model.enums import aggregation_source_to_str
from nautilus_trader.model.enums import aggressor_side_from_str
from nautilus_trader.model.enums import aggressor_side_to_str
from nautilus_trader.model.enums import asset_class_from_str
from nautilus_trader.model.enums import asset_class_to_str
from nautilus_trader.model.enums import bar_aggregation_from_str
from nautilus_trader.model.enums import bar_aggregation_to_str
from nautilus_trader.model.enums import book_action_from_str
from nautilus_trader.model.enums import book_action_to_str
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.enums import contingency_type_from_str
from nautilus_trader.model.enums import contingency_type_to_str
from nautilus_trader.model.enums import currency_type_from_str
from nautilus_trader.model.enums import currency_type_to_str
from nautilus_trader.model.enums import instrument_class_from_str
from nautilus_trader.model.enums import instrument_class_to_str
from nautilus_trader.model.enums import instrument_close_type_from_str
from nautilus_trader.model.enums import instrument_close_type_to_str
from nautilus_trader.model.enums import liquidity_side_from_str
from nautilus_trader.model.enums import liquidity_side_to_str
from nautilus_trader.model.enums import market_status_action_from_str
from nautilus_trader.model.enums import market_status_action_to_str
from nautilus_trader.model.enums import market_status_from_str
from nautilus_trader.model.enums import market_status_to_str
from nautilus_trader.model.enums import oms_type_from_str
from nautilus_trader.model.enums import oms_type_to_str
from nautilus_trader.model.enums import option_kind_from_str
from nautilus_trader.model.enums import option_kind_to_str
from nautilus_trader.model.enums import order_side_from_str
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.enums import order_status_from_str
from nautilus_trader.model.enums import order_status_to_str
from nautilus_trader.model.enums import order_type_from_str
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.enums import position_side_from_str
from nautilus_trader.model.enums import position_side_to_str
from nautilus_trader.model.enums import price_type_from_str
from nautilus_trader.model.enums import price_type_to_str
from nautilus_trader.model.enums import record_flag_from_str
from nautilus_trader.model.enums import record_flag_to_str
from nautilus_trader.model.enums import time_in_force_from_str
from nautilus_trader.model.enums import time_in_force_to_str
from nautilus_trader.model.enums import trading_state_from_str
from nautilus_trader.model.enums import trading_state_to_str
from nautilus_trader.model.enums import trailing_offset_type_from_str
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_from_str
from nautilus_trader.model.enums import trigger_type_to_str


class TestAccountType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [AccountType.CASH, "CASH"],
            [AccountType.MARGIN, "MARGIN"],
            [AccountType.BETTING, "BETTING"],
        ],
    )
    def test_account_type_to_str(self, enum, expected):
        # Arrange, Act
        result = account_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["CASH", AccountType.CASH],
            ["MARGIN", AccountType.MARGIN],
            ["BETTING", AccountType.BETTING],
        ],
    )
    def test_account_type_from_str(self, string, expected):
        # Arrange, Act
        result = account_type_from_str(string)

        # Assert
        assert result == expected

    def test_instantiate_from_string(self):
        assert AccountType["CASH"] == AccountType.CASH


class TestAggregationSource:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [AggregationSource.EXTERNAL, "EXTERNAL"],
            [AggregationSource.INTERNAL, "INTERNAL"],
        ],
    )
    def test_aggregation_source_to_str(self, enum, expected):
        # Arrange, Act
        result = aggregation_source_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["EXTERNAL", AggregationSource.EXTERNAL],
            ["INTERNAL", AggregationSource.INTERNAL],
        ],
    )
    def test_aggregation_source_from_str(self, string, expected):
        # Arrange, Act
        result = aggregation_source_from_str(string)

        # Assert
        assert result == expected


class TestAggressorSide:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [AggressorSide.NO_AGGRESSOR, "NO_AGGRESSOR"],
            [AggressorSide.BUYER, "BUYER"],
            [AggressorSide.SELLER, "SELLER"],
        ],
    )
    def test_aggressor_side_to_str(self, enum, expected):
        # Arrange, Act
        result = aggressor_side_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_AGGRESSOR", AggressorSide.NO_AGGRESSOR],
            ["BUYER", AggressorSide.BUYER],
            ["SELLER", AggressorSide.SELLER],
        ],
    )
    def test_aggressor_side_from_str(self, string, expected):
        # Arrange, Act
        result = aggressor_side_from_str(string)

        # Assert
        assert result == expected


class TestAssetClass:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [AssetClass.FX, "FX"],
            [AssetClass.EQUITY, "EQUITY"],
            [AssetClass.COMMODITY, "COMMODITY"],
            [AssetClass.DEBT, "DEBT"],
            [AssetClass.INDEX, "INDEX"],
            [AssetClass.CRYPTOCURRENCY, "CRYPTOCURRENCY"],
            [AssetClass.ALTERNATIVE, "ALTERNATIVE"],
        ],
    )
    def test_asset_class_to_str(self, enum, expected):
        # Arrange, Act
        result = asset_class_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["FX", AssetClass.FX],
            ["EQUITY", AssetClass.EQUITY],
            ["COMMODITY", AssetClass.COMMODITY],
            ["DEBT", AssetClass.DEBT],
            ["INDEX", AssetClass.INDEX],
            ["CRYPTOCURRENCY", AssetClass.CRYPTOCURRENCY],
            ["ALTERNATIVE", AssetClass.ALTERNATIVE],
        ],
    )
    def test_asset_class_from_str(self, string, expected):
        # Arrange, Act
        result = asset_class_from_str(string)

        # Assert
        assert result == expected


class TestInstrumentClass:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [InstrumentClass.SPOT, "SPOT"],
            [InstrumentClass.SWAP, "SWAP"],
            [InstrumentClass.FUTURE, "FUTURE"],
            [InstrumentClass.FUTURES_SPREAD, "FUTURES_SPREAD"],
            [InstrumentClass.FORWARD, "FORWARD"],
            [InstrumentClass.CFD, "CFD"],
            [InstrumentClass.BOND, "BOND"],
            [InstrumentClass.OPTION, "OPTION"],
            [InstrumentClass.OPTION_SPREAD, "OPTION_SPREAD"],
            [InstrumentClass.WARRANT, "WARRANT"],
            [InstrumentClass.SPORTS_BETTING, "SPORTS_BETTING"],
            [InstrumentClass.BINARY_OPTION, "BINARY_OPTION"],
        ],
    )
    def test_instrument_class_to_str(self, enum, expected):
        # Arrange, Act
        result = instrument_class_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["SPOT", InstrumentClass.SPOT],
            ["SWAP", InstrumentClass.SWAP],
            ["FUTURE", InstrumentClass.FUTURE],
            ["FUTURES_SPREAD", InstrumentClass.FUTURES_SPREAD],
            ["FORWARD", InstrumentClass.FORWARD],
            ["CFD", InstrumentClass.CFD],
            ["BOND", InstrumentClass.BOND],
            ["OPTION", InstrumentClass.OPTION],
            ["OPTION_SPREAD", InstrumentClass.OPTION_SPREAD],
            ["WARRANT", InstrumentClass.WARRANT],
            ["SPORTS_BETTING", InstrumentClass.SPORTS_BETTING],
            ["BINARY_OPTION", InstrumentClass.BINARY_OPTION],
        ],
    )
    def test_instrument_class_from_str(self, string, expected):
        # Arrange, Act
        result = instrument_class_from_str(string)

        # Assert
        assert result == expected


class TestBarAggregation:
    @pytest.mark.parametrize(
        ("enum", "expected"),
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
        result = bar_aggregation_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
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
        result = bar_aggregation_from_str(string)

        # Assert
        assert result == expected


class TestBookAction:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [BookAction.ADD, "ADD"],
            [BookAction.UPDATE, "UPDATE"],
            [BookAction.DELETE, "DELETE"],
            [BookAction.CLEAR, "CLEAR"],
        ],
    )
    def test_book_action_to_str(self, enum, expected):
        # Arrange, Act
        result = book_action_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["ADD", BookAction.ADD],
            ["UPDATE", BookAction.UPDATE],
            ["DELETE", BookAction.DELETE],
            ["CLEAR", BookAction.CLEAR],
        ],
    )
    def test_book_action_from_str(self, string, expected):
        # Arrange, Act
        if expected is None:
            return

        result = book_action_from_str(string)

        # Assert
        assert result == expected


class TestBookType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [BookType.L1_MBP, "L1_MBP"],
            [BookType.L2_MBP, "L2_MBP"],
            [BookType.L3_MBO, "L3_MBO"],
        ],
    )
    def test_orderbook_level_to_str(self, enum, expected):
        # Arrange, Act
        result = book_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["", None],
            ["L1_MBP", BookType.L1_MBP],
            ["L2_MBP", BookType.L2_MBP],
            ["L3_MBO", BookType.L3_MBO],
        ],
    )
    def test_orderbook_level_from_str(self, string, expected):
        # Arrange, Act
        if expected is None:
            return

        result = book_type_from_str(string)

        # Assert
        assert result == expected


class TestContingencyType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [ContingencyType.NO_CONTINGENCY, "NO_CONTINGENCY"],
            [ContingencyType.OCO, "OCO"],
            [ContingencyType.OTO, "OTO"],
            [ContingencyType.OUO, "OUO"],
        ],
    )
    def test_contingency_type_to_str(self, enum, expected):
        # Arrange, Act
        result = contingency_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_CONTINGENCY", ContingencyType.NO_CONTINGENCY],
            ["OCO", ContingencyType.OCO],
            ["OTO", ContingencyType.OTO],
            ["OUO", ContingencyType.OUO],
        ],
    )
    def test_contingency_type_from_str(self, string, expected):
        # Arrange, Act
        result = contingency_type_from_str(string)

        # Assert
        assert result == expected


class TestCurrencyType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [CurrencyType.CRYPTO, "CRYPTO"],
            [CurrencyType.FIAT, "FIAT"],
        ],
    )
    def test_currency_type_to_str(self, enum, expected):
        # Arrange, Act
        result = currency_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["CRYPTO", CurrencyType.CRYPTO],
            ["FIAT", CurrencyType.FIAT],
        ],
    )
    def test_currency_type_from_str(self, string, expected):
        # Arrange, Act
        result = currency_type_from_str(string)

        # Assert
        assert result == expected


class TestOptionKind:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [OptionKind.CALL, "CALL"],
            [OptionKind.PUT, "PUT"],
        ],
    )
    def test_option_kind_to_str(self, enum, expected):
        # Arrange, Act
        result = option_kind_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["CALL", OptionKind.CALL],
            ["PUT", OptionKind.PUT],
        ],
    )
    def test_option_kind_from_str(self, string, expected):
        # Arrange, Act
        result = option_kind_from_str(string)

        # Assert
        assert result == expected


class TestInstrumentCloseType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [InstrumentCloseType.END_OF_SESSION, "END_OF_SESSION"],
            [InstrumentCloseType.CONTRACT_EXPIRED, "CONTRACT_EXPIRED"],
        ],
    )
    def test_instrument_close_type_to_str(self, enum, expected):
        # Arrange, Act
        result = instrument_close_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["END_OF_SESSION", InstrumentCloseType.END_OF_SESSION],
            ["CONTRACT_EXPIRED", InstrumentCloseType.CONTRACT_EXPIRED],
        ],
    )
    def test_instrument_close_type_from_str(self, string, expected):
        # Arrange, Act
        result = instrument_close_type_from_str(string)

        # Assert
        assert result == expected


class TestLiquiditySide:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [LiquiditySide.NO_LIQUIDITY_SIDE, "NO_LIQUIDITY_SIDE"],
            [LiquiditySide.MAKER, "MAKER"],
            [LiquiditySide.TAKER, "TAKER"],
        ],
    )
    def test_liquidity_side_to_str(self, enum, expected):
        # Arrange, Act
        result = liquidity_side_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_LIQUIDITY_SIDE", LiquiditySide.NO_LIQUIDITY_SIDE],
            ["MAKER", LiquiditySide.MAKER],
            ["TAKER", LiquiditySide.TAKER],
        ],
    )
    def test_liquidity_side_from_str(self, string, expected):
        # Arrange, Act
        result = liquidity_side_from_str(string)

        # Assert
        assert result == expected


class TestMarketStatus:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [MarketStatus.OPEN, "OPEN"],
            [MarketStatus.CLOSED, "CLOSED"],
            [MarketStatus.PAUSED, "PAUSED"],
            [MarketStatus.SUSPENDED, "SUSPENDED"],
            [MarketStatus.NOT_AVAILABLE, "NOT_AVAILABLE"],
        ],
    )
    def test_market_status_to_str(self, enum, expected):
        # Arrange, Act
        result = market_status_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["OPEN", MarketStatus.OPEN],
            ["CLOSED", MarketStatus.CLOSED],
            ["PAUSED", MarketStatus.PAUSED],
            ["SUSPENDED", MarketStatus.SUSPENDED],
            ["NOT_AVAILABLE", MarketStatus.NOT_AVAILABLE],
        ],
    )
    def test_market_status_from_str(self, string, expected):
        # Arrange, Act
        result = market_status_from_str(string)

        # Assert
        assert result == expected


class TestMarketStatusAction:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [MarketStatusAction.NONE, "NONE"],
            [MarketStatusAction.PRE_OPEN, "PRE_OPEN"],
            [MarketStatusAction.PRE_CROSS, "PRE_CROSS"],
            [MarketStatusAction.QUOTING, "QUOTING"],
            [MarketStatusAction.CROSS, "CROSS"],
            [MarketStatusAction.ROTATION, "ROTATION"],
            [MarketStatusAction.NEW_PRICE_INDICATION, "NEW_PRICE_INDICATION"],
            [MarketStatusAction.TRADING, "TRADING"],
            [MarketStatusAction.HALT, "HALT"],
            [MarketStatusAction.PAUSE, "PAUSE"],
            [MarketStatusAction.SUSPEND, "SUSPEND"],
            [MarketStatusAction.PRE_CLOSE, "PRE_CLOSE"],
            [MarketStatusAction.CLOSE, "CLOSE"],
            [MarketStatusAction.POST_CLOSE, "POST_CLOSE"],
            [MarketStatusAction.SHORT_SELL_RESTRICTION_CHANGE, "SHORT_SELL_RESTRICTION_CHANGE"],
            [MarketStatusAction.NOT_AVAILABLE_FOR_TRADING, "NOT_AVAILABLE_FOR_TRADING"],
        ],
    )
    def test_market_status_action_to_str(self, enum, expected):
        # Arrange, Act
        result = market_status_action_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NONE", MarketStatusAction.NONE],
            ["PRE_OPEN", MarketStatusAction.PRE_OPEN],
            ["PRE_CROSS", MarketStatusAction.PRE_CROSS],
            ["QUOTING", MarketStatusAction.QUOTING],
            ["CROSS", MarketStatusAction.CROSS],
            ["ROTATION", MarketStatusAction.ROTATION],
            ["NEW_PRICE_INDICATION", MarketStatusAction.NEW_PRICE_INDICATION],
            ["TRADING", MarketStatusAction.TRADING],
            ["HALT", MarketStatusAction.HALT],
            ["PAUSE", MarketStatusAction.PAUSE],
            ["SUSPEND", MarketStatusAction.SUSPEND],
            ["PRE_CLOSE", MarketStatusAction.PRE_CLOSE],
            ["CLOSE", MarketStatusAction.CLOSE],
            ["POST_CLOSE", MarketStatusAction.POST_CLOSE],
            ["SHORT_SELL_RESTRICTION_CHANGE", MarketStatusAction.SHORT_SELL_RESTRICTION_CHANGE],
            ["NOT_AVAILABLE_FOR_TRADING", MarketStatusAction.NOT_AVAILABLE_FOR_TRADING],
        ],
    )
    def test_market_status_action_from_str(self, string, expected):
        # Arrange, Act
        result = market_status_action_from_str(string)

        # Assert
        assert result == expected


class TestOmsType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [OmsType.UNSPECIFIED, "UNSPECIFIED"],
            [OmsType.NETTING, "NETTING"],
            [OmsType.HEDGING, "HEDGING"],
        ],
    )
    def test_oms_type_to_str(self, enum, expected):
        # Arrange, Act
        result = oms_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["UNSPECIFIED", OmsType.UNSPECIFIED],
            ["NETTING", OmsType.NETTING],
            ["HEDGING", OmsType.HEDGING],
        ],
    )
    def test_oms_type_from_str(self, string, expected):
        # Arrange, Act
        result = oms_type_from_str(string)

        # Assert
        assert result == expected


class TestOrderSide:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [OrderSide.NO_ORDER_SIDE, "NO_ORDER_SIDE"],
            [OrderSide.BUY, "BUY"],
            [OrderSide.SELL, "SELL"],
        ],
    )
    def test_order_side_to_str(self, enum, expected):
        # Arrange, Act
        result = order_side_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_ORDER_SIDE", OrderSide.NO_ORDER_SIDE],
            ["BUY", OrderSide.BUY],
            ["SELL", OrderSide.SELL],
        ],
    )
    def test_order_side_from_str(self, string, expected):
        # Arrange, Act
        result = order_side_from_str(string)

        # Assert
        assert result == expected


class TestOrderStatus:
    @pytest.mark.parametrize(
        ("enum", "expected"),
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
        result = order_status_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
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
        result = order_status_from_str(string)

        # Assert
        assert result == expected


class TestOrderType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
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
        result = order_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
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
        result = order_type_from_str(string)

        # Assert
        assert result == expected


class TestRecordFlag:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [RecordFlag.F_LAST, "F_LAST"],
            [RecordFlag.F_TOB, "F_TOB"],
            [RecordFlag.F_SNAPSHOT, "F_SNAPSHOT"],
            [RecordFlag.F_MBP, "F_MBP"],
        ],
    )
    def test_record_flag_to_str(self, enum, expected):
        # Arrange, Act
        result = record_flag_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["F_LAST", RecordFlag.F_LAST],
            ["F_TOB", RecordFlag.F_TOB],
            ["F_SNAPSHOT", RecordFlag.F_SNAPSHOT],
            ["F_MBP", RecordFlag.F_MBP],
        ],
    )
    def test_record_flag_from_str(self, string, expected):
        # Arrange, Act
        result = record_flag_from_str(string)

        # Assert
        assert result == expected


class TestPositionSide:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [PositionSide.NO_POSITION_SIDE, "NO_POSITION_SIDE"],
            [PositionSide.FLAT, "FLAT"],
            [PositionSide.LONG, "LONG"],
            [PositionSide.SHORT, "SHORT"],
        ],
    )
    def test_position_side_to_str(self, enum, expected):
        # Arrange, Act
        result = position_side_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_POSITION_SIDE", PositionSide.NO_POSITION_SIDE],
            ["FLAT", PositionSide.FLAT],
            ["LONG", PositionSide.LONG],
            ["SHORT", PositionSide.SHORT],
        ],
    )
    def test_position_side_from_str(self, string, expected):
        # Arrange, Act
        result = position_side_from_str(string)

        # Assert
        assert result == expected


class TestPriceType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [PriceType.BID, "BID"],
            [PriceType.ASK, "ASK"],
            [PriceType.MID, "MID"],
            [PriceType.LAST, "LAST"],
            [PriceType.MARK, "MARK"],
        ],
    )
    def test_price_type_to_str(self, enum, expected):
        # Arrange, Act
        result = price_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["BID", PriceType.BID],
            ["ASK", PriceType.ASK],
            ["MID", PriceType.MID],
            ["LAST", PriceType.LAST],
            ["MARK", PriceType.MARK],
        ],
    )
    def test_price_type_from_str(self, string, expected):
        # Arrange, Act
        result = price_type_from_str(string)

        # Assert
        assert result == expected


class TestTimeInForce:
    @pytest.mark.parametrize(
        ("enum", "expected"),
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
        result = time_in_force_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
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
        result = time_in_force_from_str(string)

        # Assert
        assert result == expected


class TestTradingState:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [TradingState.ACTIVE, "ACTIVE"],
            [TradingState.HALTED, "HALTED"],
            [TradingState.REDUCING, "REDUCING"],
        ],
    )
    def test_trading_state_to_str(self, enum, expected):
        # Arrange, Act
        result = trading_state_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["ACTIVE", TradingState.ACTIVE],
            ["HALTED", TradingState.HALTED],
            ["REDUCING", TradingState.REDUCING],
        ],
    )
    def test_trading_state_from_str(self, string, expected):
        # Arrange, Act
        result = trading_state_from_str(string)

        # Assert
        assert result == expected


class TestTrailingOffsetType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [TrailingOffsetType.NO_TRAILING_OFFSET, "NO_TRAILING_OFFSET"],
            [TrailingOffsetType.PRICE, "PRICE"],
            [TrailingOffsetType.BASIS_POINTS, "BASIS_POINTS"],
            [TrailingOffsetType.TICKS, "TICKS"],
            [TrailingOffsetType.PRICE_TIER, "PRICE_TIER"],
        ],
    )
    def test_trailing_offset_type_to_str(self, enum, expected):
        # Arrange, Act
        result = trailing_offset_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_TRAILING_OFFSET", TrailingOffsetType.NO_TRAILING_OFFSET],
            ["PRICE", TrailingOffsetType.PRICE],
            ["BASIS_POINTS", TrailingOffsetType.BASIS_POINTS],
            ["TICKS", TrailingOffsetType.TICKS],
            ["PRICE_TIER", TrailingOffsetType.PRICE_TIER],
        ],
    )
    def test_trailing_offset_type_from_str(self, string, expected):
        # Arrange, Act
        result = trailing_offset_type_from_str(string)

        # Assert
        assert result == expected


class TestTriggerType:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [TriggerType.NO_TRIGGER, "NO_TRIGGER"],
            [TriggerType.DEFAULT, "DEFAULT"],
            [TriggerType.LAST_PRICE, "LAST_PRICE"],
            [TriggerType.BID_ASK, "BID_ASK"],
            [TriggerType.DOUBLE_LAST, "DOUBLE_LAST"],
            [TriggerType.DOUBLE_BID_ASK, "DOUBLE_BID_ASK"],
            [TriggerType.LAST_OR_BID_ASK, "LAST_OR_BID_ASK"],
            [TriggerType.MID_POINT, "MID_POINT"],
            [TriggerType.MARK_PRICE, "MARK_PRICE"],
            [TriggerType.INDEX_PRICE, "INDEX_PRICE"],
        ],
    )
    def test_trigger_type_to_str(self, enum, expected):
        # Arrange, Act
        result = trigger_type_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["NO_TRIGGER", TriggerType.NO_TRIGGER],
            ["DEFAULT", TriggerType.DEFAULT],
            ["LAST_PRICE", TriggerType.LAST_PRICE],
            ["BID_ASK", TriggerType.BID_ASK],
            ["DOUBLE_LAST", TriggerType.DOUBLE_LAST],
            ["DOUBLE_BID_ASK", TriggerType.DOUBLE_BID_ASK],
            ["LAST_OR_BID_ASK", TriggerType.LAST_OR_BID_ASK],
            ["MID_POINT", TriggerType.MID_POINT],
            ["MARK_PRICE", TriggerType.MARK_PRICE],
            ["INDEX_PRICE", TriggerType.INDEX_PRICE],
        ],
    )
    def test_trigger_type_from_str(self, string, expected):
        # Arrange, Act
        result = trigger_type_from_str(string)

        # Assert
        assert result == expected
