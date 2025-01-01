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

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType


class OKXTradeFee(msgspec.Struct):
    category: str  # deprecated
    delivery: str  # Delivery fee rate
    exercise: str  # Fee rate for exercising the option
    instType: OKXInstrumentType
    level: str  # Fee rate Level
    maker: str  # maker fee rate
    makerU: str  # maker for USDT-margined contracts
    makerUSDC: str  # maker for USDC-margined instruments
    taker: str  # taker fee rate
    takerU: str  # taker for USDT-margined instruments
    takerUSDC: str  # taker for USDC-margined instruments
    ts: str  # Unix timestamp in milliseconds
    fiat: list  # Details of fiat fee rate?


class OKXTradeFeeResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXTradeFee]
