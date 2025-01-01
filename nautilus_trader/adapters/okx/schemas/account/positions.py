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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXTriggerType
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity


class OKXCloseOrderAlgoData(msgspec.Struct):
    algoId: str
    slTriggerPx: str
    slTriggerPxType: OKXTriggerType
    tpTriggerPx: str
    tpTriggerPxType: OKXTriggerType
    closeFraction: str  # fraction of position to be closed when algo order is triggered


class OKXAccountPositionData(msgspec.Struct):
    instType: OKXInstrumentType
    mgnMode: OKXMarginMode
    posId: str
    posSide: OKXPositionSide
    pos: str  # qty of positions
    baseBal: str  # DEPRECATED
    quoteBal: str  # DEPRECATED
    baseBorrowed: str  # DEPRECATED
    baseInterest: str  # DEPRECATED
    quoteBorrowed: str  # DEPRECATED
    quoteInterest: str  # DEPRECATED
    posCcy: str  # position currency applicable to margin positions
    availPos: str  # position that can be closed, applicable to MARGIN/FUTURES/SWAP long/short mode
    avgPx: str  # avg open price
    markPx: str  # latest mark price
    upl: str  # unrealized pnl calculated by mark price
    uplRatio: str  # unrealized pnl ratio calc'd by mark price
    uplLastPx: str  # Unrealized pbl calculated by last price. For show, actual value is upl
    uplRatioLastPx: str  # unrealized pnl ratio calc'd by last price
    instId: str
    lever: str  # leverage
    liqPx: str  # estimated liquidation price
    imr: str  # init margin rqt, only applicable to 'cross'
    margin: str  # margin, can be added or reduced, only applicable to 'isolated'
    mgnRatio: str  # margin ratio
    mmr: str  # maint margin rqt
    liab: str  # liabilities, only applicable to MARGIN
    liabCcy: str  # liabilities currency, only applicable to MARGIN
    interest: str  # interest. Undeducted interest that has been incurred
    tradeId: str  # last trade id
    optVal: str  # Option value, only applicable to OPTION
    pendingCloseOrdLiabVal: str  # amount of close orders of isolated margin liability
    notionalUsd: str  # notional value of positions in USD
    adl: str  # auto-deleveraging indicator, 1-5 in increasing risk of adl
    ccy: str  # currency used for margin
    last: str  # last traded price
    idxPx: str  # last underlying index price
    usdPx: str  # last USD price of the ccy on the market, only applicable to OPTION
    bePx: str  # breakeven price
    deltaBS: str  # black-scholes delta in USD, only applicable to OPTION
    deltaPA: str  # black-scholes delta in coins, only applicable to OPTION
    gammaBS: str  # black-scholes gamma in USD, only applicable to OPTION
    gammaPA: str  # black-scholes gamma in coins, only applicable to OPTION
    thetaBS: str  # black-scholes theta in USD, only applicable to OPTION
    thetaPA: str  # black-scholes theta in coins, only applicable to OPTION
    vegaBS: str  # black-scholes vega in USD, only applicable to OPTION
    vegaPA: str  # black-scholes vega in coins, only applicable to OPTION
    spotInUseAmt: str  # spot in use amount, applicable to portfolio margin
    spotInUseCcy: str  # spot in use unit, eg BTC, applicable to portfolio margin
    clSpotInUseAmt: str  # user-defined spot risk offset amount, applicable to portfolio margin
    maxSpotInUseAmt: str  # max possible spot risk offset amount, applicable to portfolio margin
    bizRefId: str  # external business id, eg experience coupon id
    bizRefType: str  # external business type
    realizedPnl: str  # realized pnl, applicable to FUTURES/SWAP/OPTION, pnll+fee+fundingFee+liqPen
    pnl: str  # accumulated pnl of closing order(s)
    fee: str  # accumulated fee, negative means user tx fee charged by platform, positive is rebate
    fundingFee: str  # accumulated funding fee
    liqPenalty: str  # accumulated liquidation penalty, negative when present
    closeOrderAlgo: list[OKXCloseOrderAlgoData]
    cTime: str
    uTime: str
    # pTime: str  # push time of positions; NOTE not present in http positions endpoint

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        position_side = self.posSide.parse_to_position_side(self.pos)
        size = Quantity.from_str(self.pos.removeprefix("-"))  # Quantity does not accept negatives
        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=size,
            report_id=report_id,
            ts_init=ts_init,
            ts_last=millis_to_nanos(Decimal(self.uTime)),
        )


class OKXAccountPositionsResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXAccountPositionData]
