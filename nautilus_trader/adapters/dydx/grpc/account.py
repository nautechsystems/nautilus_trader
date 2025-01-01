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
Implement the client for GRPC account endpoints.
"""

import asyncio
import hashlib
from functools import partial

import bech32
import ecdsa
import google
import grpc
import msgspec
from bip_utils import Bip39SeedGenerator
from bip_utils import Bip44
from bip_utils import Bip44Coins
from Crypto.Hash import RIPEMD160
from ecdsa.util import sigencode_string_canonize
from google._upb._message import Message
from v4_proto.cosmos.auth.v1beta1 import query_pb2_grpc as auth
from v4_proto.cosmos.auth.v1beta1.auth_pb2 import BaseAccount
from v4_proto.cosmos.auth.v1beta1.query_pb2 import QueryAccountRequest
from v4_proto.cosmos.bank.v1beta1 import query_pb2 as bank_query
from v4_proto.cosmos.bank.v1beta1 import query_pb2_grpc as bank_query_grpc
from v4_proto.cosmos.base.tendermint.v1beta1 import query_pb2 as tendermint_query
from v4_proto.cosmos.base.tendermint.v1beta1 import query_pb2_grpc as tendermint_query_grpc
from v4_proto.cosmos.base.v1beta1.coin_pb2 import Coin
from v4_proto.cosmos.crypto.secp256k1.keys_pb2 import PubKey
from v4_proto.cosmos.tx.signing.v1beta1.signing_pb2 import SignMode
from v4_proto.cosmos.tx.v1beta1 import service_pb2_grpc
from v4_proto.cosmos.tx.v1beta1.service_pb2 import BroadcastMode
from v4_proto.cosmos.tx.v1beta1.service_pb2 import BroadcastTxRequest
from v4_proto.cosmos.tx.v1beta1.service_pb2 import BroadcastTxResponse
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import AuthInfo
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import Fee
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import ModeInfo
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import SignDoc
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import SignerInfo
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import Tx
from v4_proto.cosmos.tx.v1beta1.tx_pb2 import TxBody
from v4_proto.dydxprotocol.clob.order_pb2 import Order
from v4_proto.dydxprotocol.clob.order_pb2 import OrderId
from v4_proto.dydxprotocol.clob.tx_pb2 import MsgBatchCancel
from v4_proto.dydxprotocol.clob.tx_pb2 import MsgCancelOrder
from v4_proto.dydxprotocol.clob.tx_pb2 import MsgPlaceOrder
from v4_proto.dydxprotocol.clob.tx_pb2 import OrderBatch
from v4_proto.dydxprotocol.feetiers import query_pb2 as fee_tier_query
from v4_proto.dydxprotocol.feetiers import query_pb2_grpc as fee_tier_query_grpc
from v4_proto.dydxprotocol.subaccounts.subaccount_pb2 import SubaccountId

from nautilus_trader.adapters.dydx.common.constants import ACCOUNT_SEQUENCE_MISMATCH_ERROR_CODE
from nautilus_trader.adapters.dydx.grpc.errors import DYDXGRPCError


DEFAULT_FEE = Fee(
    amount=[],
    gas_limit=1000000,
)


from_string = partial(ecdsa.SigningKey.from_string, curve=ecdsa.SECP256k1, hashfunc=hashlib.sha256)


def as_any(message: Message) -> google.protobuf.any_pb2.Any:
    """
    Wrap the message in an Any container.
    """
    packed = google.protobuf.any_pb2.Any()
    packed.Pack(message, type_url_prefix="/")
    return packed


def get_signer_info(public_key: PubKey, sequence: int) -> SignerInfo:
    """
    Construct a SignerInfo instance.
    """
    return SignerInfo(
        public_key=as_any(public_key),
        mode_info=ModeInfo(single=ModeInfo.Single(mode=SignMode.SIGN_MODE_DIRECT)),
        sequence=sequence,
    )


def get_signature(
    private_key: ecdsa.SigningKey,
    body: TxBody,
    auth_info: AuthInfo,
    account_number: int,
    chain_id: str,
) -> bytes:
    """
    Create the signature for the transaction with the private signing key.
    """
    signdoc = SignDoc(
        body_bytes=body.SerializeToString(),
        auth_info_bytes=auth_info.SerializeToString(),
        account_number=account_number,
        chain_id=chain_id,
    )

    return private_key.sign(signdoc.SerializeToString(), sigencode=sigencode_string_canonize)


def bytes_from_mnemonic(mnemonic: str) -> bytes:
    """
    Create a Bib44 private signing key.
    """
    seed = Bip39SeedGenerator(mnemonic).Generate()
    return Bip44.FromSeed(seed, Bip44Coins.COSMOS).DeriveDefaultPath().PrivateKey().Raw().ToBytes()


def from_mnemonic(mnemonic: str) -> ecdsa.SigningKey:
    """
    Generate a private signing key from a mnemonic.
    """
    return from_string(bytes_from_mnemonic(mnemonic))


class Wallet:
    """
    Store the private key and account number in the wallet.
    """

    def __init__(self, mnemonic: str, account_number: int, sequence: int) -> None:
        """
        Store the private key and account number in the wallet.
        """
        self.key = from_mnemonic(mnemonic)
        self.account_number = account_number
        self.sequence = sequence

    @property
    def public_key(self) -> PubKey:
        """
        Return the public key.
        """
        return PubKey(key=self.key.get_verifying_key().to_string("compressed"))

    @property
    def address(self) -> str:
        """
        Return the public address.
        """
        public_key_bytes = self.public_key.key
        sha256_hash = hashlib.sha256(public_key_bytes).digest()
        ripemd160_hash = RIPEMD160.new(sha256_hash).digest()
        return bech32.bech32_encode("dydx", bech32.convertbits(ripemd160_hash, 8, 5))


class TransactionBuilder:
    """
    Create signed transactions to place orders on the dYdX chain.
    """

    def __init__(self, chain_id: str, denomination: str, memo: str | None = None) -> None:
        """
        Create a new transaction builder.
        """
        self.chain_id = chain_id
        self.denomination = denomination
        self.memo = memo

    def coin(self, amount: int) -> Coin:
        """
        Return the coin.
        """
        return Coin(amount=str(amount), denom=self.denomination)

    def fee(self, gas_limit: int, *amount: list[Coin]) -> Fee:
        """
        Determine the fee for the transaction.
        """
        return Fee(
            amount=amount,
            gas_limit=gas_limit,
        )

    def build_transaction(self, wallet: Wallet, messages: list[Message], fee: Fee) -> Tx:
        """
        Build the transaction.
        """
        body = TxBody(messages=messages, memo=self.memo)
        auth_info = AuthInfo(
            signer_infos=[get_signer_info(wallet.public_key, wallet.sequence)],
            fee=fee,
        )
        signature = get_signature(wallet.key, body, auth_info, wallet.account_number, self.chain_id)

        return Tx(body=body, auth_info=auth_info, signatures=[signature])

    def build(self, wallet: Wallet, message: Message, fee: Fee = DEFAULT_FEE) -> Tx:
        """
        Build the transaction.
        """
        return self.build_transaction(wallet, [as_any(message)], fee)


class DYDXAccountGRPCAPI:
    """
    Define the account GRPC API endpoints.
    """

    def __init__(self, channel_url: str, transaction_builder: TransactionBuilder) -> None:
        """
        Define the account GRPC API endpoints.
        """
        self._channel_url = channel_url

        grpc_service_config = msgspec.json.encode(
            {
                "methodConfig": [
                    {
                        "name": [{}],  # match all RPCs
                        "retryPolicy": {
                            "maxAttempts": 5,
                            "initialBackoff": "0.1s",
                            "maxBackoff": "10s",
                            "backoffMultiplier": 2,
                            "retryableStatusCodes": ["UNAVAILABLE"],
                        },
                    },
                ],
            },
        ).decode()

        self._channel: grpc.aio.Channel = grpc.aio.secure_channel(
            target=self._channel_url,
            credentials=grpc.ssl_channel_credentials(),
            options=[("grpc.service_config", grpc_service_config)],
        )
        self._transaction_builder = transaction_builder
        self._lock = asyncio.Lock()

    async def connect(self) -> None:
        """
        Connect to the GRPC server.
        """

    async def disconnect(self) -> None:
        """
        Disconnect from the GRPC server.
        """
        await self._channel.close()

    async def get_account(self, address: str) -> BaseAccount:
        """
        Retrieve the account information for a given address.

        Parameters
        ----------
        address : str
            The account address.

        Returns
        -------
        BaseAccount
            The base account information.

        """
        account = BaseAccount()
        response = await auth.QueryStub(self._channel).Account(QueryAccountRequest(address=address))

        if not response.account.Unpack(account):
            message = "Failed to unpack account"
            raise DYDXGRPCError(code=None, message=message)

        return account

    async def get_account_balances(self, address: str) -> bank_query.QueryAllBalancesResponse:
        """
        Retrieve all account balances for a given address.

        Parameters
        ----------
        address : str
            The account address.

        Returns
        -------
        bank_query.QueryAllBalancesResponse
            The response containing all account balances.

        """
        stub = bank_query_grpc.QueryStub(self._channel)
        return await stub.AllBalances(bank_query.QueryAllBalancesRequest(address=address))

    async def latest_block(self) -> tendermint_query.GetLatestBlockResponse:
        """
        Retrieve the latest block information.

        Returns
        -------
        tendermint_query.GetLatestBlockResponse
            The response containing the latest block information.

        """
        return await tendermint_query_grpc.ServiceStub(self._channel).GetLatestBlock(
            tendermint_query.GetLatestBlockRequest(),
        )

    async def latest_block_height(self) -> int:
        """
        Retrieve the height of the latest block.

        Returns
        -------
        int
            The height of the latest block.

        """
        block = await self.latest_block()
        return block.block.header.height

    async def get_fee_tiers(self) -> fee_tier_query.QueryPerpetualFeeParamsResponse:
        """
        Retrieve the perpetual fee parameters.

        Returns
        -------
        fee_tier_query.QueryPerpetualFeeParamsResponse
            The response containing the perpetual fee parameters.

        """
        stub = fee_tier_query_grpc.QueryStub(self._channel)
        return await stub.PerpetualFeeParams(fee_tier_query.QueryPerpetualFeeParamsRequest())

    async def get_user_fee_tier(self, address: str) -> fee_tier_query.QueryUserFeeTierResponse:
        """
        Retrieve the user fee tier for a given address.

        Parameters
        ----------
        address : str
            The user address.

        Returns
        -------
        fee_tier_query.QueryUserFeeTierResponse
            The response containing the user fee tier.

        """
        stub = fee_tier_query_grpc.QueryStub(self._channel)
        return await stub.UserFeeTier(fee_tier_query.QueryUserFeeTierRequest(user=address))

    async def place_order(self, wallet: Wallet, order: Order) -> BroadcastTxResponse:
        """
        Places an order.

        Parameters
        ----------
        wallet : Wallet
            The wallet to use for signing the transaction.
        order : Order
            The order to place.

        Returns
        -------
        BroadcastTxResponse
            The response from the transaction broadcast.

        """
        response = await self.broadcast_message(wallet, MsgPlaceOrder(order=order))

        is_success = response.tx_response.code == 0

        if not is_success:
            message = f"Failed to place the order: {response}"
            raise DYDXGRPCError(code=response.tx_response.code, message=message)

        return response

    async def batch_cancel_orders(
        self,
        wallet: Wallet,
        wallet_address: str,
        subaccount: int,
        short_term_cancels: list[OrderBatch],
        good_til_block: int,
    ) -> BroadcastTxResponse:
        """
        Batch cancels orders for a subaccount.

        Parameters
        ----------
        wallet : Wallet
            The wallet to use for signing the transaction.
        wallet_address : str
            The dYdX wallet address.
        subaccount : int
            The subaccount number.
        short_term_cancels : list[OrderBatch]
            List of OrderBatch objects containing the orders to cancel.
        good_til_block : int
            The last block the short term order cancellations can be executed at.

        Returns
        -------
        BroadcastTxResponse
            The response from the transaction broadcast.

        """
        subaccount_id = SubaccountId(owner=wallet_address, number=subaccount)
        batch_cancel_msg = MsgBatchCancel(
            subaccount_id=subaccount_id,
            short_term_cancels=short_term_cancels,
            good_til_block=good_til_block,
        )
        response = await self.broadcast_message(wallet, batch_cancel_msg)

        is_success = response.tx_response.code == 0

        if not is_success:
            message = f"Failed to cancel the orders: {response}"
            raise DYDXGRPCError(code=response.tx_response.code, message=message)

        return response

    async def cancel_order(
        self,
        wallet: Wallet,
        order_id: OrderId,
        good_til_block: int | None = None,
        good_til_block_time: int | None = None,
    ) -> BroadcastTxResponse:
        """
        Cancel an order.

        Parameters
        ----------
        wallet : Wallet
            The wallet to use for signing the transaction.
        order_id : OrderId
            The ID of the order to cancel.
        good_til_block : int, optional
            The block number until which the order is valid. Defaults to None.
        good_til_block_time: int, optional
            The block time until which the order is valid. Defaults to None.

        Returns
        -------
        BroadcastTxResponse
            The response from the transaction broadcast.

        """
        message = MsgCancelOrder(
            order_id=order_id,
            good_til_block=good_til_block,
            good_til_block_time=good_til_block_time,
        )
        response = await self.broadcast_message(wallet, message)

        is_success = response.tx_response.code == 0

        if not is_success:
            message = f"Failed to cancel the order: {response}"
            raise DYDXGRPCError(code=response.tx_response.code, message=message)

        return response

    async def broadcast_message(
        self,
        wallet: Wallet,
        message: Message,
        mode: BroadcastMode = BroadcastMode.BROADCAST_MODE_SYNC,
    ) -> BroadcastTxResponse:
        """
        Broadcast a message.

        Parameters
        ----------
        wallet : Wallet
            The wallet to use for signing the transaction.
        message : Message
            The message to broadcast.
        mode : BroadcastMode, optional
            The broadcast mode. Defaults to BroadcastMode.BROADCAST_MODE_SYNC.

        Returns
        -------
            The response from the broadcast.

        """
        async with self._lock:
            response = await self.broadcast(self._transaction_builder.build(wallet, message), mode)

            if response.tx_response.code == 0:
                wallet.sequence += 1

            # The sequence number is not correct. Retrieve it from the gRPC channel.
            # The retry manager can retry the transaction.
            elif response.tx_response.code == ACCOUNT_SEQUENCE_MISMATCH_ERROR_CODE:
                account = await self.get_account(wallet.address)
                wallet.sequence = account.sequence

            return response

    async def broadcast(
        self,
        transaction: Tx,
        mode: BroadcastMode = BroadcastMode.BROADCAST_MODE_SYNC,
    ) -> BroadcastTxResponse:
        """
        Broadcast a transaction.

        Parameters
        ----------
        transaction : Tx
            The transaction to broadcast.
        mode : BroadcastMode, optional
            The broadcast mode. Defaults to BroadcastMode.BROADCAST_MODE_SYNC.

        Returns
        -------
        BroadcastTxResponse
            The response from the broadcast.

        """
        request = BroadcastTxRequest(tx_bytes=transaction.SerializeToString(), mode=mode)

        return await service_pb2_grpc.ServiceStub(self._channel).BroadcastTx(request)
