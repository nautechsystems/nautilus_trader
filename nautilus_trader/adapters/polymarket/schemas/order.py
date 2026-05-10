# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketSignatureType


class PolymarketOrder(msgspec.Struct, frozen=True):
    """
    Represents a Polymarket CLOB V2 limit order wire body.

    References
    ----------
    https://docs.polymarket.com/api-reference/trade/post-a-new-order

    """

    salt: int  # random salt used to create a unique order
    maker: str  # maker address (funder)
    signer: str  # signed address
    tokenId: str  # ERC1155 token ID of the conditional token being traded
    makerAmount: str  # maximum amount maker is willing to spend
    takerAmount: str  # maximum amount taker is willing to spend
    expiration: str  # UNIX expiration timestamp (seconds); "0" for non-GTD orders
    side: PolymarketOrderSide
    signatureType: PolymarketSignatureType
    timestamp: str  # order creation time in milliseconds (replaces v1 nonce)
    metadata: str  # bytes32 metadata
    builder: str  # bytes32 builder code
    signature: str  # hex encoded string


class PolymarketMakerOrder(msgspec.Struct, frozen=True, omit_defaults=True):
    """
    Represents a Polymarket maker order (included for trades).

    `side` is included on CLOB V2 REST trade responses; legacy or
    user-channel WS payloads may omit it, hence the optional default.
    `omit_defaults=True` ensures the absent-side case round-trips through
    `to_dict()` without injecting `"side": null` into fill metadata.

    References
    ----------
    https://docs.polymarket.com/api-reference/trade/get-trades

    """

    asset_id: str
    fee_rate_bps: str
    maker_address: str
    matched_amount: str
    order_id: str
    outcome: str
    owner: str
    price: str
    side: PolymarketOrderSide | None = None
