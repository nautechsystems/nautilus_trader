import msgspec

from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketSignatureType


class PolymarketOrder(msgspec.Struct, frozen=True):
    """
    Represents a Polymarket limit order.

    References
    ----------
    https://docs.polymarket.com/#create-and-place-an-order

    """

    salt: int  # random salt used to create unique order
    maker: str  # maker address (funder)
    signer: str  # signed address
    taker: str  # taker address (operator)
    tokenId: str  # ERC1155 token ID of conditional token being traded
    makerAmount: str  # maximum amount maker is willing to spend
    takerAmount: str  # maximum amount taker is willing to spend
    expiration: str  # UNIX expiration timestamp (seconds?)  # TBD
    nonce: str  # makers Exchange nonce the order is associated with
    feeRateBps: str  # fee rate in basis points as required by the operator
    side: PolymarketOrderSide
    signatureType: PolymarketSignatureType  # signature
    signature: str  # hex encoded string


class PolymarketMakerOrder(msgspec.Struct, frozen=True):
    """
    Represents a Polymarket maker order (included for trades).

    References
    ----------
    https://docs.polymarket.com/#user-channel

    """

    asset_id: str
    fee_rate_bps: str
    maker_address: str
    matched_amount: str
    order_id: str
    outcome: str
    owner: str
    price: str
