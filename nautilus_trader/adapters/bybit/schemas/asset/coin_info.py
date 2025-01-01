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

from typing import Any

import msgspec

from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.objects import Currency


class BybitCoinChainInfo(msgspec.Struct):
    confirmation: str
    chainType: str
    withdrawFee: str
    depositMin: str
    withdrawMin: str
    chain: str
    chainDeposit: str
    chainWithdraw: str
    minAccuracy: str
    withdrawPercentageFee: str


class BybitCoinInfo(msgspec.Struct):
    name: str
    coin: str
    remainAmount: str
    chains: list[BybitCoinChainInfo]

    def parse_to_currency(self) -> Currency:
        return Currency(
            code=self.coin,
            name=self.coin,
            currency_type=CurrencyType.CRYPTO,
            precision=int(self.chains[0].minAccuracy),
            iso4217=0,  # Currently unspecified for crypto assets
        )


class BybitCoinInfoResult(msgspec.Struct):
    rows: list[BybitCoinInfo]


class BybitCoinInfoResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitCoinInfoResult
    retExtInfo: dict[str, Any]
    time: int
