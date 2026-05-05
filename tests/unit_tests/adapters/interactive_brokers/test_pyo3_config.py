# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.core import nautilus_pyo3


def test_data_config_uses_clean_connection_names() -> None:
    ib = nautilus_pyo3.interactive_brokers

    config = ib.InteractiveBrokersDataClientConfig(
        host="127.0.0.2",
        port=4002,
        client_id=12,
        request_timeout=33,
    )

    assert config.host == "127.0.0.2"
    assert config.port == 4002
    assert config.client_id == 12
    assert config.request_timeout == 33


def test_exec_config_uses_clean_connection_names() -> None:
    ib = nautilus_pyo3.interactive_brokers

    config = ib.InteractiveBrokersExecClientConfig(
        host="127.0.0.3",
        port=4003,
        client_id=13,
        request_timeout=34,
    )

    assert config.host == "127.0.0.3"
    assert config.port == 4003
    assert config.client_id == 13
    assert config.request_timeout == 34


def test_exec_config_rejects_client_id_multiple_of_1000() -> None:
    ib = nautilus_pyo3.interactive_brokers

    with pytest.raises(ValueError, match="must not be a multiple of 1000"):
        ib.InteractiveBrokersExecClientConfig(client_id=1000)


def test_pyo3_config_rejects_unwired_dockerized_gateway() -> None:
    ib = nautilus_pyo3.interactive_brokers

    gateway = ib.DockerizedIBGatewayConfig(trading_mode=ib.TradingMode.PAPER)

    with pytest.raises(ValueError, match="not wired into the Rust/PyO3 IB data client"):
        ib.InteractiveBrokersDataClientConfig(dockerized_gateway=gateway)

    with pytest.raises(ValueError, match="not wired into the Rust/PyO3 IB execution client"):
        ib.InteractiveBrokersExecClientConfig(dockerized_gateway=gateway)


def test_market_data_type_class_attrs_are_config_values() -> None:
    ib = nautilus_pyo3.interactive_brokers

    config = ib.InteractiveBrokersDataClientConfig(
        market_data_type=ib.MarketDataType.DELAYED_FROZEN,
    )

    assert config.market_data_type == ib.MarketDataType.DELAYED_FROZEN


def test_trading_mode_class_attrs_are_config_values() -> None:
    ib = nautilus_pyo3.interactive_brokers

    config = ib.DockerizedIBGatewayConfig(trading_mode=ib.TradingMode.PAPER)

    assert config.trading_mode == ib.TradingMode.PAPER


def test_adapter_config_and_status_enums_are_python_accessible() -> None:
    ib = nautilus_pyo3.interactive_brokers

    provider_config = ib.InteractiveBrokersInstrumentProviderConfig(
        symbology_method=ib.SymbologyMethod.RAW,
    )

    assert provider_config.symbology_method == ib.SymbologyMethod.RAW
    assert ib.ContainerStatus.READY == ib.ContainerStatus.READY
    assert ib.ErrorCategory.CONNECTIVITY_ERROR.as_str() == "ConnectivityError"
    assert ib.InteractiveBrokersErrorKind.IB_API.as_str() == "IbApi"


def test_ib_adapter_enums_are_python_accessible() -> None:
    ib = nautilus_pyo3.interactive_brokers

    assert ib.IbAction.BUY.as_str() == "BUY"
    assert ib.IbAction.SELL_SHORT.as_str() == "SSHORT"
    assert ib.IbOrderStatus.SUBMITTED.as_str() == "Submitted"
    assert ib.IbOrderType.TRAILING_STOP_LIMIT.as_str() == "TRAIL LIMIT"
    assert ib.IbOrderType.PEGGED_TO_MIDPOINT.as_str() == "PEG MID"
    assert ib.IbTimeInForce.GOOD_TIL_CANCELED.as_str() == "GTC"
    assert ib.IbBuilderTimeInForce.GOOD_TILL_CROSSING.as_str() == "GTX"
    assert ib.IbSecurityType.FUTURES_OPTION.as_str() == "FOP"
    assert ib.IbOptionRight.CALL.as_str() == "C"
    assert ib.IbTickType.DELAYED_ASK.as_i32() == 67
    assert ib.IbHistoricalTickType.BID_ASK.as_str() == "BID_ASK"
    assert ib.IbTradingHours.REGULAR.use_rth() is True
    assert ib.IbHistoricalBarSize.MIN5.as_str() == "5 mins"
    assert ib.IbHistoricalWhatToShow.ADJUSTED_LAST.as_str() == "ADJUSTED_LAST"
    assert ib.IbRealtimeBarSize.SEC5.as_str() == "5 secs"
    assert ib.IbRealtimeWhatToShow.MIDPOINT.as_str() == "MIDPOINT"
    assert ib.IbOrderOrigin.FIRM.as_i32() == 1
    assert ib.IbShortSaleSlot.THIRD_PARTY.as_i32() == 2
    assert ib.IbVolatilityType.ANNUAL.as_i32() == 2
    assert ib.IbReferencePriceType.NBBO.as_i32() == 2
    assert ib.IbRule80A.AGENCY_PT.as_str() == "Y"
    assert ib.IbAuctionStrategy.TRANSPARENT.as_i32() == 3
    assert ib.IbOrderOpenClose.CLOSE.as_str() == "C"
    assert ib.IbExerciseAction.LAPSE.as_i32() == 2
    assert ib.IbAuctionType.VOLATILITY.as_i32() == 4
    assert ib.IbTwapStrategyType.MATCHING_LAST.as_str() == "Matching Last"
    assert ib.IbRiskAversion.GET_DONE.as_str() == "Get Done"
    assert ib.IbLegAction.SELL.as_str() == "SELL"
    assert ib.IbFundDistributionPolicyIndicator.INCOME_FUND.as_str() == "Y"
    assert ib.IbFundAssetType.FIXED_INCOME.as_str() == "002"
    assert ib.IbArticleType.BINARY.as_i32() == 1
    assert ib.IbBondIdentifierKind.ISIN.as_str() == "ISIN"
    assert ib.IbPlaceOrderEvent.COMMISSION_REPORT.as_str() == "CommissionReport"
    assert ib.IbOrderUpdateEvent.EXECUTION_DATA.as_str() == "ExecutionData"
    assert ib.IbCancelOrderEvent.NOTICE.as_str() == "Notice"
    assert ib.IbOrdersEvent.ORDER_DATA.as_str() == "OrderData"
    assert ib.IbExecutionsEvent.COMMISSION_REPORT.as_str() == "CommissionReport"
    assert ib.IbExerciseOptionsEvent.OPEN_ORDER.as_str() == "OpenOrder"
    assert ib.IbHistoricalBarUpdateEvent.END.as_str() == "End"
    assert ib.IbMarketDepthEvent.MARKET_DEPTH_L2.as_str() == "MarketDepthL2"
    assert ib.IbTickEvent.OPTION_COMPUTATION.as_str() == "OptionComputation"
    assert ib.IbAccountSummaryEvent.SUMMARY.as_str() == "Summary"
    assert ib.IbPositionUpdateEvent.POSITION_END.as_str() == "PositionEnd"
    assert ib.IbPositionUpdateMultiEvent.POSITION.as_str() == "Position"
    assert ib.IbAccountUpdateEvent.PORTFOLIO_VALUE.as_str() == "PortfolioValue"
    assert ib.IbAccountUpdateMultiEvent.ACCOUNT_MULTI_VALUE.as_str() == "AccountMultiValue"
    assert ib.IbConditionKind.PERCENT_CHANGE.as_str() == "percent_change"
    assert ib.IbConditionConjunction.AND.is_conjunction() is True
    assert ib.IbComboLegOpenClose.OPEN.as_i32() == 1
    assert ib.IbTriggerMethod.LAST_OR_BID_ASK.as_i32() == 7
    assert ib.IbOcaType.CANCEL_WITH_BLOCK.as_i32() == 1
    assert ib.IbLiquidity.REMOVED_LIQUIDITY.as_i32() == 2
