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

from typing import Final

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


BYBIT: Final[str] = "BYBIT"
BYBIT_VENUE: Final[Venue] = Venue(BYBIT)
BYBIT_CLIENT_ID: Final[ClientId] = ClientId(BYBIT)

BYBIT_ALL_PRODUCTS: Final[list[BybitProductType]] = [
    BybitProductType.SPOT,
    BybitProductType.LINEAR,
    BybitProductType.INVERSE,
    BybitProductType.OPTION,
]

# Set of Bybit error codes for which Nautilus will attempt retries,
# potentially temporary conditions where a retry might make sense.
BYBIT_RETRY_ERRORS_UTA: Final[set[int]] = {
    # > ------------------------------------------------------------
    # > Self defined error codes
    -10_408,  # Client request timed out
    # > ------------------------------------------------------------
    # > Bybit defined error codes
    # > https://bybit-exchange.github.io/docs/v5/error
    10_000,  # Server Timeout
    10_002,  # The request time exceeds the time window range
    10_006,  # Too many visits. Exceeded the API Rate Limit
    10_016,  # Server error
    40_004,  # The order is modified during the process of replacing
    110_009,  # The number of stop orders exceeds the maximum allowable limit
    110_011,  # Liquidation will be triggered immediately by this adjustment
    110_017,  # Reduce-only rule not satisfied
    110_020,  # Not allowed to have more than 500 active orders
    110_021,  # Not allowed to exceeded position limits due to Open Interest
    110_022,  # Quantity has been restricted and orders cannot be modified to increase the quantity
    110_024,  # You have an existing position, so the position mode cannot be switched
    110_028,  # You have existing open orders, so the position mode cannot be switched
    110_034,  # There is no net position
    110_040,  # The order will trigger a forced liquidation, please re-submit the order
    110_041,  # Skip liquidation is not allowed when a position or maker order exists
    110_042,  # Currently,due to pre-delivery status, you can only reduce your position on this contract
    110_044,  # Available margin is insufficient
    110_045,  # Wallet balance is insufficient
    110_046,  # Liquidation will be triggered immediately by this adjustment
    110_047,  # Risk limit cannot be adjusted due to insufficient available margin
    110_048,  # Risk limit cannot be adjusted as the current/expected position value exceeds the revised risk limit
    110_051,  # The user's available balance cannot cover the lowest price of the current market
    110_052,  # Your available balance is insufficient to set the price
    110_053,  # The user's available balance cannot cover the current market price and upper limit price
    110_054,  # This position has at least one take profit link order, so the take profit and stop loss mode cannot be switched
    110_055,  # This position has at least one stop loss link order, so the take profit and stop loss mode cannot be switched
    110_056,  # This position has at least one trailing stop link order, so the take profit and stop loss mode cannot be switched
    110_058,  # You can't set take profit and stop loss due to insufficient size of remaining position size
    110_061,  # Not allowed to have more than 20 TP/SLs under Partial tpSlMode
    110_063,  # Settlement in progress, not available for trading
    110_066,  # Trading is currently not allowed
    110_079,  # The order is processing and can not be operated
    110_080,  # Operations Restriction: The current LTV ratio of your Institutional Lending has hit the liquidation threshold. Assets in your account are being liquidated (trade/risk limit/leverage)
    110_082,  # You cannot lift Reduce-Only restrictions, as no Reduce-Only restrictions are applied to your position
    110_083,  # Reduce-Only restrictions must be lifted for both Long and Short positions at the same time
    110_089,  # Exceeds the maximum risk limit level
    110_090,  # Exceeds the maximum leverage limit of the current risk limit level
    181_017,  # OrderStatus must be final status
    182_101,  # Failed repayment, insufficient collateral balance
    3_400_052,  # You have uncancelled USDC perpetual orders
    3_400_053,  # You have uncancelled Options orders
    3_400_054,  # You have uncancelled USDT perpetual orders
    3_400_214,  # Server error, please try again later
    3_400_071,  # The net asset is not satisfied
    3_400_139,  # The total value of your positions and orders has exceeded the risk limit for a Perpetual or Futures contract
}

BYBIT_MINUTE_INTERVALS: Final[tuple[int, ...]] = (1, 3, 5, 15, 30, 60, 120, 240, 360, 720)
BYBIT_HOUR_INTERVALS: Final[tuple[int, ...]] = (1, 2, 4, 6, 12)

BYBIT_SPOT_DEPTHS: Final[tuple[int, ...]] = (1, 50, 200)
BYBIT_LINEAR_DEPTHS: Final[tuple[int, ...]] = (1, 50, 200, 500)
BYBIT_INVERSE_DEPTHS: Final[tuple[int, ...]] = (1, 50, 200, 500)
BYBIT_OPTION_DEPTHS: Final[tuple[int, ...]] = (25, 100)
