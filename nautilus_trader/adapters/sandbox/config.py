# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.config import LiveExecClientConfig


class SandboxExecutionClientConfig(LiveExecClientConfig, frozen=True, kw_only=True):
    """
    Configuration for ``SandboxExecClient`` instances.

    Parameters
    ----------
    venue : str
        The venue to generate a sandbox execution client for.
    currency : str
        The currency for this venue.
    balance : int
        The starting balance for this venue.
    bar_execution : bool, default True
        If bars should be processed by the matching engine(s) (and move the market).
    default_leverage : float, default 1.0
        The account default leverage (for margin accounts).
    oms_type : str, default 'NETTING'
        The order management system type used by the exchange.
    account_type : str, default 'MARGIN'
        The account type for the client.

    """

    venue: str
    starting_balances: list[str]
    base_currency: str | None = None
    oms_type: str = "NETTING"
    account_type: str = "MARGIN"
    default_leverage: float = 1.0
    leverages: dict[str, float] | None = None
    book_type: str = "L1_MBP"
    frozen_account: bool = False
    bar_execution: bool = True
    reject_stop_orders: bool = True
    support_gtd_orders: bool = True
    support_contingent_orders: bool = True
    use_position_ids: bool = True
    use_random_ids: bool = False
    use_reduce_only: bool = True
