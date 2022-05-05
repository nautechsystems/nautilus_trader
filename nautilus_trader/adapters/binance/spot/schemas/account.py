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

from typing import List

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType


################################################################################
# HTTP responses
################################################################################


class BinanceSpotBalanceInfo(msgspec.Struct):
    """
    HTTP response 'inner struct' from `Binance Spot/Margin` GET /api/v3/account (HMAC SHA256).
    """

    asset: str
    free: str
    locked: str


class BinanceSpotAccountInfo(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin` GET /api/v3/account (HMAC SHA256).
    """

    makerCommission: int
    takerCommission: int
    buyerCommission: int
    sellerCommission: int
    canTrade: bool
    canWithdraw: bool
    canDeposit: bool
    updateTime: int
    accountType: BinanceAccountType
    balances: List[BinanceSpotBalanceInfo]
    permissions: List[str]
