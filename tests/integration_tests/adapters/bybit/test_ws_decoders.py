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

import pkgutil

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitExecType
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitPositionIdx
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitStopOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountExecution
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountExecutionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountOrder
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountOrderMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountPosition
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountPositionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountWallet
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountWalletCoin
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountWalletMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsKline
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsKlineMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsLiquidation
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsLiquidationMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderbookDepth
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderbookDepthMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerLinear
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerLinearMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerOption
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerOptionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerSpot
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerSpotMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTrade
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTradeMsg
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class TestBybitWsDecoders:
    def test_ws_public_kline(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_kline.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsKlineMsg)
        target_kline = BybitWsKline(
            start=1672324800000,
            end=1672325099999,
            interval=BybitKlineInterval.MINUTE_5,
            open="16649.5",
            close="16677",
            high="16677",
            low="16608",
            volume="2.081",
            turnover="34666.4005",
            confirm=False,
            timestamp=1672324988882,
        )
        result = decoder.decode(item)
        assert result.data == [target_kline]
        assert result.topic == "kline.5.BTCUSDT"
        assert result.ts == 1672324988882
        assert result.type == "snapshot"

    def test_ws_public_liquidation(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_liquidation.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsLiquidationMsg)
        result = decoder.decode(item)
        target_liquidation = BybitWsLiquidation(
            price="0.03803",
            side=BybitOrderSide.BUY,
            size="1637",
            symbol="GALAUSDT",
            updatedTime=1673251091822,
        )
        assert result.data == target_liquidation
        assert result.topic == "liquidation.GALAUSDT"
        assert result.ts == 1673251091822
        assert result.type == "snapshot"

    def test_ws_public_orderbook_delta(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_delta.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)
        result = decoder.decode(item)
        target_data = BybitWsOrderbookDepth(
            s="BTCUSDT",
            b=[
                ["30247.20", "30.028"],
                ["30245.40", "0.224"],
                ["30242.10", "1.593"],
                ["30240.30", "1.305"],
                ["30240.00", "0"],
            ],
            a=[
                ["30248.70", "0"],
                ["30249.30", "0.892"],
                ["30249.50", "1.778"],
                ["30249.60", "0"],
                ["30251.90", "2.947"],
                ["30252.20", "0.659"],
                ["30252.50", "4.591"],
            ],
            u=177400507,
            seq=66544703342,
        )
        assert result.data == target_data
        assert result.topic == "orderbook.50.BTCUSDT"
        assert result.ts == 1687940967466
        assert result.type == "delta"

    def test_ws_public_orderbook_delta_parse_to_deltas(self):
        # Prepare
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_delta.json",
        )
        assert item is not None
        instrument_id = InstrumentId(Symbol("BTCUSDT-LINEAR"), Venue("BYBIT"))
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)

        # Act
        result = decoder.decode(item).data.parse_to_deltas(
            instrument_id=instrument_id,
            price_precision=2,
            size_precision=2,
            ts_event=0,
            ts_init=0,
        )

        # Assert
        assert len(result.deltas) == 12
        assert result.is_snapshot is False

        # Test that only the last delta has a F_LAST flag
        for delta_id, delta in enumerate(result.deltas):
            if delta_id < len(result.deltas) - 1:
                assert delta.flags == 0
            else:
                assert delta.flags == RecordFlag.F_LAST

    def test_ws_public_orderbook_delta_parse_to_deltas_no_asks(self):
        # Prepare
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_delta_no_asks.json",
        )
        assert item is not None
        instrument_id = InstrumentId(Symbol("BTCUSDT-LINEAR"), Venue("BYBIT"))
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)

        # Act
        result = decoder.decode(item).data.parse_to_deltas(
            instrument_id=instrument_id,
            price_precision=2,
            size_precision=2,
            ts_event=0,
            ts_init=0,
        )

        # Assert
        assert len(result.deltas) == 5
        assert result.is_snapshot is False

        # Test that only the last delta has a F_LAST flag
        for delta_id, delta in enumerate(result.deltas):
            if delta_id < len(result.deltas) - 1:
                assert delta.flags == 0
            else:
                assert delta.flags == RecordFlag.F_LAST

    def test_ws_public_orderbook_snapshot(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_snapshot.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)
        result = decoder.decode(item)
        target_data = BybitWsOrderbookDepth(
            s="BTCUSDT",
            b=[
                ["16493.50", "0.006"],
                ["16493.00", "0.100"],
            ],
            a=[
                ["16611.00", "0.029"],
                ["16612.00", "0.213"],
            ],
            u=18521288,
            seq=7961638724,
        )
        assert result.data == target_data
        assert result.topic == "orderbook.50.BTCUSDT"
        assert result.type == "snapshot"
        assert result.ts == 1672304484978

    def test_ws_public_orderbook_snapshot_flags(self):
        # Prepare
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_snapshot.json",
        )
        assert item is not None
        instrument_id = InstrumentId(Symbol("BTCUSDT-LINEAR"), Venue("BYBIT"))
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)

        # Act
        result = decoder.decode(item).data.parse_to_deltas(
            instrument_id=instrument_id,
            price_precision=2,
            size_precision=2,
            ts_event=0,
            ts_init=0,
            snapshot=True,
        )

        # Assert
        assert len(result.deltas) == 5
        assert result.is_snapshot

        # Test that only the last delta has a F_LAST flag
        for delta_id, delta in enumerate(result.deltas):
            if delta_id < len(result.deltas) - 1:
                assert delta.flags == 0
            else:
                assert delta.flags == RecordFlag.F_LAST

    def test_ws_public_orderbook_snapshot_flags_no_asks(self):
        # Prepare
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_orderbook_snapshot_no_asks.json",
        )
        assert item is not None
        instrument_id = InstrumentId(Symbol("BTCUSDT-LINEAR"), Venue("BYBIT"))
        decoder = msgspec.json.Decoder(BybitWsOrderbookDepthMsg)

        # Act
        result = decoder.decode(item).data.parse_to_deltas(
            instrument_id=instrument_id,
            price_precision=2,
            size_precision=2,
            ts_event=0,
            ts_init=0,
            snapshot=True,
        )

        # Assert
        assert len(result.deltas) == 3
        assert result.is_snapshot

        # Test that only the last delta has a F_LAST flag
        for delta_id, delta in enumerate(result.deltas):
            if delta_id < len(result.deltas) - 1:
                assert delta.flags == 0
            else:
                assert delta.flags == RecordFlag.F_LAST

    def test_ws_public_ticker_linear(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_ticker_linear.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsTickerLinearMsg)
        result = decoder.decode(item)
        target_data = BybitWsTickerLinear(
            symbol="BTCUSDT",
            tickDirection="PlusTick",
            price24hPcnt="0.017103",
            lastPrice="17216.00",
            prevPrice24h="16926.50",
            highPrice24h="17281.50",
            lowPrice24h="16915.00",
            prevPrice1h="17238.00",
            markPrice="17217.33",
            indexPrice="17227.36",
            openInterest="68744.761",
            openInterestValue="1183601235.91",
            turnover24h="1570383121.943499",
            volume24h="91705.276",
            nextFundingTime="1673280000000",
            fundingRate="-0.000212",
            bid1Price="17215.50",
            bid1Size="84.489",
            ask1Price="17216.00",
            ask1Size="83.020",
        )
        assert result.data == target_data
        assert result.topic == "tickers.BTCUSDT"
        assert result.type == "snapshot"
        assert result.ts == 1673272861686
        assert result.cs == 24987956059

    def test_ws_public_ticker_option(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_ticker_option.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsTickerOptionMsg)
        result = decoder.decode(item)
        target_data = BybitWsTickerOption(
            symbol="BTC-6JAN23-17500-C",
            bidPrice="0",
            bidSize="0",
            bidIv="0",
            askPrice="10",
            askSize="5.1",
            askIv="0.514",
            lastPrice="10",
            highPrice24h="25",
            lowPrice24h="5",
            markPrice="7.86976724",
            indexPrice="16823.73",
            markPriceIv="0.4896",
            underlyingPrice="16815.1",
            openInterest="49.85",
            turnover24h="446802.8473",
            volume24h="26.55",
            totalVolume="86",
            totalTurnover="1437431",
            delta="0.047831",
            gamma="0.00021453",
            vega="0.81351067",
            theta="-19.9115368",
            predictedDeliveryPrice="0",
            change24h="-0.33333334",
        )
        assert result.data == target_data
        assert result.topic == "tickers.BTC-6JAN23-17500-C"
        assert result.type == "snapshot"
        assert result.ts == 1672917511074

    def test_ws_public_ticker_spot(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_ticker_spot.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsTickerSpotMsg)
        result = decoder.decode(item)
        target_data = BybitWsTickerSpot(
            symbol="BTCUSDT",
            lastPrice="21109.77",
            highPrice24h="21426.99",
            lowPrice24h="20575",
            prevPrice24h="20704.93",
            volume24h="6780.866843",
            turnover24h="141946527.22907118",
            price24hPcnt="0.0196",
            usdIndexPrice="21120.2400136",
        )
        assert result.data == target_data
        assert result.topic == "tickers.BTCUSDT"
        assert result.type == "snapshot"
        assert result.ts == 1673853746003
        assert result.cs == 2588407389

    def test_ws_public_trade(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_trade.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsTradeMsg)
        result = decoder.decode(item)
        target_trade = BybitWsTrade(
            T=1672304486865,
            s="BTCUSDT",
            S="Buy",
            v="0.001",
            p="16578.50",
            L="PlusTick",
            i="20f43950-d8dd-5b31-9112-a178eb6023af",
            BT=False,
        )
        assert result.data == [target_trade]
        assert result.topic == "publicTrade.BTCUSDT"
        assert result.type == "snapshot"
        assert result.ts == 1672304486868

    def test_ws_trade_msg_parse_to_trade_tick(self):
        # Prepare
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.public",
            "ws_trade.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsTradeMsg)
        instrument_id = InstrumentId(Symbol("BTCUSDT-LINEAR"), Venue("BYBIT"))
        expected_result = TradeTick(
            instrument_id=instrument_id,
            price=Price(16578.50, 3),
            size=Quantity(0.001, 4),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("20f43950-d8dd-5b31-9112-a178eb6023af"),
            ts_event=1672304486864999936,
            ts_init=1672304486864999937,
        )

        # Act
        result = (
            decoder.decode(item)
            .data[0]
            .parse_to_trade_tick(
                instrument_id=instrument_id,
                price_precision=3,
                size_precision=4,
                ts_init=1672304486864999937,
            )
        )

        # Assert
        assert result == expected_result

    def test_ws_private_execution(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.private",
            "ws_execution.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsAccountExecutionMsg)
        result = decoder.decode(item)
        target_data = BybitWsAccountExecution(
            category=BybitProductType.LINEAR,
            symbol="XRPUSDT",
            execFee="0.005061",
            execId="7e2ae69c-4edf-5800-a352-893d52b446aa",
            execPrice="0.3374",
            execQty="25",
            execType=BybitExecType("Trade"),
            execValue="8.435",
            isMaker=False,
            feeRate="0.0006",
            tradeIv="",
            markIv="",
            blockTradeId="",
            markPrice="0.3391",
            indexPrice="",
            underlyingPrice="",
            leavesQty="0",
            orderId="f6e324ff-99c2-4e89-9739-3086e47f9381",
            orderLinkId="",
            orderPrice="0.3207",
            orderQty="25",
            orderType=BybitOrderType.MARKET,
            stopOrderType=BybitStopOrderType("UNKNOWN"),
            side=BybitOrderSide.SELL,
            execTime="1672364174443",
            isLeverage="0",
            closedSize="",
            seq=4688002127,
        )
        assert result.data == [target_data]
        assert result.topic == "execution"
        assert result.id == "592324803b2785-26fa-4214-9963-bdd4727f07be"
        assert result.creationTime == 1672364174455

    def test_ws_private_order(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.private",
            "ws_order.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsAccountOrderMsg)
        result = decoder.decode(item)
        target_data = BybitWsAccountOrder(
            category=BybitProductType.OPTION,
            symbol="ETH-30DEC22-1400-C",
            orderId="5cf98598-39a7-459e-97bf-76ca765ee020",
            side=BybitOrderSide.SELL,
            orderType=BybitOrderType.MARKET,
            cancelType="UNKNOWN",
            price="72.5",
            qty="1",
            orderIv="",
            timeInForce=BybitTimeInForce.IOC,
            orderStatus=BybitOrderStatus.FILLED,
            orderLinkId="",
            lastPriceOnCreated="",
            reduceOnly=False,
            leavesQty="",
            leavesValue="",
            cumExecQty="1",
            cumExecValue="75",
            avgPrice="75",
            blockTradeId="",
            positionIdx=0,
            cumExecFee="0.358635",
            createdTime="1672364262444",
            updatedTime="1672364262457",
            rejectReason="EC_NoError",
            stopOrderType=BybitStopOrderType.NONE,
            tpslMode="",
            triggerPrice="",
            takeProfit="",
            stopLoss="",
            tpTriggerBy="",
            slTriggerBy="",
            tpLimitPrice="",
            slLimitPrice="",
            triggerDirection=BybitTriggerDirection.RISES_TO,
            triggerBy=BybitTriggerType.NONE,
            closeOnTrigger=False,
            placeType="price",
            smpType="None",
            smpGroup=0,
            smpOrderId="",
            feeCurrency="",
        )
        assert result.data == [target_data]
        assert result.topic == "order"
        assert result.id == "5923240c6880ab-c59f-420b-9adb-3639adc9dd90"
        assert result.creationTime == 1672364262474

    def test_ws_private_position(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.private",
            "ws_position.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsAccountPositionMsg)
        result = decoder.decode(item)
        target_data = BybitWsAccountPosition(
            positionIdx=BybitPositionIdx.ONE_WAY,
            tradeMode=0,
            riskId=41,
            riskLimitValue="200000",
            symbol="XRPUSDT",
            side=BybitOrderSide.BUY,
            size="75",
            entryPrice="0.3615",
            leverage="10",
            positionValue="27.1125",
            positionBalance="0",
            markPrice="0.3374",
            positionIM="2.72589075",
            positionMM="0.28576575",
            takeProfit="0",
            stopLoss="0",
            trailingStop="0",
            sessionAvgPrice="0",
            unrealisedPnl="-1.8075",
            cumRealisedPnl="0.64782276",
            createdTime="1672121182216",
            updatedTime="1672364174449",
            tpslMode="Full",
            liqPrice="",
            bustPrice="",
            category=BybitProductType.LINEAR,
            positionStatus="Normal",
            adlRankIndicator=2,
            autoAddMargin=0,
            leverageSysUpdatedTime="",
            mmrSysUpdatedTime="",
            isReduceOnly=False,
            seq=4688002127,
        )
        assert result.data == [target_data]
        assert result.topic == "position"
        assert result.id == "59232430b58efe-5fc5-4470-9337-4ce293b68edd"
        assert result.creationTime == 1672364174455

    def test_ws_private_wallet(self):
        item = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.ws_messages.private",
            "ws_wallet.json",
        )
        assert item is not None
        decoder = msgspec.json.Decoder(BybitWsAccountWalletMsg)
        result = decoder.decode(item)
        coin_usdc = BybitWsAccountWalletCoin(
            coin="USDC",
            equity="0",
            usdValue="0",
            walletBalance="0",
            availableToWithdraw="0",
            availableToBorrow="1500000",
            borrowAmount="0",
            accruedInterest="0",
            totalOrderIM="0",
            totalPositionIM="0",
            totalPositionMM="0",
            unrealisedPnl="0",
            cumRealisedPnl="-1100.6552094",
            bonus="0",
            collateralSwitch=True,
            marginCollateral=True,
            locked="0",
            spotHedgingQty="0",
        )
        coin_btc = BybitWsAccountWalletCoin(
            coin="BTC",
            equity="0",
            usdValue="0",
            walletBalance="0",
            availableToWithdraw="0",
            availableToBorrow="3",
            borrowAmount="0",
            accruedInterest="0",
            totalOrderIM="0",
            totalPositionIM="0",
            totalPositionMM="0",
            unrealisedPnl="0",
            cumRealisedPnl="0",
            bonus="0",
            collateralSwitch=False,
            marginCollateral=True,
            locked="0",
            spotHedgingQty="0",
        )
        coin_usdt = BybitWsAccountWalletCoin(
            coin="USDT",
            equity="4.93036732",
            usdValue="4.9306623",
            walletBalance="106.03036732",
            availableToWithdraw="0",
            availableToBorrow="2500000",
            borrowAmount="0",
            accruedInterest="0",
            totalOrderIM="7539.624",
            totalPositionIM="1179.1604584",
            totalPositionMM="61.4170584",
            unrealisedPnl="-101.1",
            cumRealisedPnl="-55295.06268939",
            bonus="0",
            collateralSwitch=True,
            marginCollateral=True,
            locked="0",
            spotHedgingQty="0",
        )
        wallet_data = BybitWsAccountWallet(
            accountIMRate="0.4782",
            accountMMRate="0.0151",
            totalEquity="19620.93864593",
            totalWalletBalance="18331.93856433",
            totalMarginBalance="18230.83251552",
            totalAvailableBalance="9511.52641225",
            totalPerpUPL="-101.10604881",
            totalInitialMargin="8719.30610327",
            totalMaintenanceMargin="277.05763376",
            coin=[coin_usdc, coin_btc, coin_usdt],
            accountLTV="0",
            accountType="UNIFIED",
        )
        assert result.data == [wallet_data]
        assert result.topic == "wallet"
        assert result.id == "5923248e5d0ee3-faeb-4864-87e4-9cd63f785c1b"
        assert result.creationTime == 1690873065683
