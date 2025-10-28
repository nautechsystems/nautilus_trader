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

from nautilus_trader.adapters.bybit.schemas.account.balance import BybitCoinBalance
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalance
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountWalletCoin


def test_bybit_coin_balance_without_spot_borrow():
    """
    Test that balance calculation works correctly when there's no spot borrow.
    """
    coin_balance = BybitCoinBalance(
        availableToBorrow="5000",
        bonus="0",
        accruedInterest="0",
        availableToWithdraw="1000.00",
        totalOrderIM="0",
        equity="1000.00",
        usdValue="1000.00",
        borrowAmount="0",
        totalPositionMM="0",
        totalPositionIM="0",
        walletBalance="1000.00",
        unrealisedPnl="0",
        cumRealisedPnl="0",
        locked="0",
        collateralSwitch=True,
        marginCollateral=True,
        coin="USDT",
        spotHedgingQty=None,
        spotBorrow=None,  # No spot borrow
    )

    account_balance = coin_balance.parse_to_account_balance()

    # Verify: actual_balance = walletBalance - 0 = 1000
    assert account_balance.total.as_decimal() == Decimal("1000.00")
    assert account_balance.locked.as_decimal() == Decimal("0")
    assert account_balance.free.as_decimal() == Decimal("1000.00")


def test_bybit_ws_wallet_coin_with_spot_borrow():
    """
    Test that WebSocket wallet updates correctly handle spot borrow.
    """
    ws_coin = BybitWsAccountWalletCoin(
        coin="BTC",
        equity="1.0",
        usdValue="50000",
        walletBalance="1.5",
        availableToWithdraw="0.8",
        availableToBorrow="2.0",
        borrowAmount="0.5",
        accruedInterest="0.001",
        totalOrderIM="0.2",
        totalPositionIM="0",
        totalPositionMM="0",
        unrealisedPnl="0",
        cumRealisedPnl="0.1",
        bonus="0",
        collateralSwitch=True,
        marginCollateral=True,
        locked="0.2",
        spotHedgingQty="0",
        spotBorrow="0.5",  # Borrowed 0.5 BTC
    )

    account_balance = ws_coin.parse_to_account_balance()

    # Verify: actual_balance = walletBalance - spotBorrow = 1.5 - 0.5 = 1.0
    assert account_balance.total.as_decimal() == Decimal("1.0")
    assert account_balance.locked.as_decimal() == Decimal("0.2")
    assert account_balance.free.as_decimal() == Decimal("0.8")


def test_bybit_wallet_balance_list_with_spot_borrow():
    """
    Test that wallet balance list correctly processes multiple coins with spot borrow.
    """
    wallet = BybitWalletBalance(
        accountType="UNIFIED",
        totalEquity="51000.00",
        totalWalletBalance="51200.00",
        totalMarginBalance="51000.00",
        totalAvailableBalance="50000.00",
        totalPerpUPL="0",
        totalInitialMargin="1000.00",
        totalMaintenanceMargin="500.00",
        accountIMRate="0.02",
        accountMMRate="0.01",
        accountLTV="0.5",
        coin=[
            BybitCoinBalance(
                availableToBorrow="5000",
                bonus="0",
                accruedInterest="0",
                availableToWithdraw="50000.00",
                totalOrderIM="0",
                equity="50000.00",
                usdValue="50000.00",
                borrowAmount="0",
                totalPositionMM="0",
                totalPositionIM="0",
                walletBalance="50000.00",
                unrealisedPnl="0",
                cumRealisedPnl="0",
                locked="0",
                collateralSwitch=True,
                marginCollateral=True,
                coin="USDT",
                spotHedgingQty=None,
                spotBorrow=None,  # No borrow for USDT
            ),
            BybitCoinBalance(
                availableToBorrow="2",
                bonus="0",
                accruedInterest="0.001",
                availableToWithdraw="0.8",
                totalOrderIM="0",
                equity="1.0",
                usdValue="1000.00",
                borrowAmount="0.2",
                totalPositionMM="0",
                totalPositionIM="0",
                walletBalance="1.2",
                unrealisedPnl="0",
                cumRealisedPnl="0",
                locked="0",
                collateralSwitch=True,
                marginCollateral=True,
                coin="BTC",
                spotHedgingQty="0",
                spotBorrow="0.2",  # Borrowed 0.2 BTC
            ),
        ],
    )

    balances = wallet.parse_to_account_balance()

    # Check USDT (no borrow)
    usdt_balance = balances[0]
    assert usdt_balance.total.as_decimal() == Decimal("50000.00")

    # Check BTC (with borrow: 1.2 - 0.2 = 1.0)
    btc_balance = balances[1]
    assert btc_balance.total.as_decimal() == Decimal("1.0")
    assert btc_balance.free.as_decimal() == Decimal("1.0")
