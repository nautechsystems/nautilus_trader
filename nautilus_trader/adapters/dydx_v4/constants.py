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
"""
Constants for the dYdX v4 adapter.
"""

from nautilus_trader.model.identifiers import Venue


DYDX = "DYDX"
DYDX_VENUE = Venue(DYDX)
DYDX_CLIENT_ID = "DYDX"

# Environment variable names for credentials
ENV_DYDX_WALLET_ADDRESS = "DYDX_WALLET_ADDRESS"
ENV_DYDX_MNEMONIC = "DYDX_MNEMONIC"
ENV_DYDX_TESTNET_WALLET_ADDRESS = "DYDX_TESTNET_WALLET_ADDRESS"
ENV_DYDX_TESTNET_MNEMONIC = "DYDX_TESTNET_MNEMONIC"
