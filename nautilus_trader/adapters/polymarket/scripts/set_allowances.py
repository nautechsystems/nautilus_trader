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

import os


# Before running this script you will need the following:
# - Install the web3 Python package (pip install -U web3==7.12.1)
# - A Polygon wallet funded with some POL

DEFAULT_POLYGON_RPC_URL = "https://polygon.drpc.org"
CHAIN_ID = 137

PUSD_COLLATERAL = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB"
CONDITIONAL_TOKENS = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"  # gitleaks:allow
CTF_EXCHANGE = "0xE111180000d2663C0091e4f400237545B87B996B"
NEG_RISK_CTF_EXCHANGE = "0xe2222d279d744050d28e00520010520000310F59"
NEG_RISK_ADAPTER = "0xadA2005600Dec949baf300f4C6120000bDB6eAab"
APPROVAL_TARGETS = (CTF_EXCHANGE, NEG_RISK_CTF_EXCHANGE, NEG_RISK_ADAPTER)
MAX_UINT256 = (1 << 256) - 1

ERC20_APPROVE_ABI = [
    {
        "constant": False,
        "inputs": [{"name": "_spender", "type": "address"}, {"name": "_value", "type": "uint256"}],
        "name": "approve",
        "outputs": [{"name": "", "type": "bool"}],
        "payable": False,
        "stateMutability": "nonpayable",
        "type": "function",
    },
]
ERC1155_SET_APPROVAL_ABI = [
    {
        "inputs": [
            {"internalType": "address", "name": "operator", "type": "address"},
            {"internalType": "bool", "name": "approved", "type": "bool"},
        ],
        "name": "setApprovalForAll",
        "outputs": [],
        "stateMutability": "nonpayable",
        "type": "function",
    },
]


def main() -> None:
    from web3 import Web3
    from web3.middleware import ExtraDataToPOAMiddleware

    private_key = os.environ["POLYGON_PRIVATE_KEY"]
    public_key = os.environ["POLYGON_PUBLIC_KEY"]
    rpc_url = os.getenv("POLYGON_RPC_URL", DEFAULT_POLYGON_RPC_URL)

    web3 = Web3(Web3.HTTPProvider(rpc_url))
    web3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
    set_allowances(web3, private_key, public_key)


def set_allowances(web3, private_key: str, public_key: str) -> None:
    public_key = web3.to_checksum_address(public_key)
    collateral = web3.eth.contract(
        address=web3.to_checksum_address(PUSD_COLLATERAL),
        abi=ERC20_APPROVE_ABI,
    )
    ctf = web3.eth.contract(
        address=web3.to_checksum_address(CONDITIONAL_TOKENS),
        abi=ERC1155_SET_APPROVAL_ABI,
    )
    nonce = web3.eth.get_transaction_count(public_key, "pending")

    for raw_target in APPROVAL_TARGETS:
        target = web3.to_checksum_address(raw_target)
        print(_approve_collateral(web3, collateral, target, private_key, public_key, nonce))
        nonce += 1
        print(_approve_ctf(web3, ctf, target, private_key, public_key, nonce))
        nonce += 1


def _approve_collateral(
    web3,
    collateral,
    target: str,
    private_key: str,
    public_key: str,
    nonce: int,
):
    transaction = collateral.functions.approve(target, MAX_UINT256).build_transaction(
        {
            "chainId": CHAIN_ID,
            "from": public_key,
            "nonce": nonce,
        },
    )
    return _send_transaction(web3, transaction, private_key)


def _approve_ctf(
    web3,
    ctf,
    target: str,
    private_key: str,
    public_key: str,
    nonce: int,
):
    transaction = ctf.functions.setApprovalForAll(target, True).build_transaction(
        {
            "chainId": CHAIN_ID,
            "from": public_key,
            "nonce": nonce,
        },
    )
    return _send_transaction(web3, transaction, private_key)


def _send_transaction(web3, transaction, private_key: str):
    signed_transaction = web3.eth.account.sign_transaction(transaction, private_key=private_key)
    transaction_hash = web3.eth.send_raw_transaction(signed_transaction.raw_transaction)
    receipt = web3.eth.wait_for_transaction_receipt(transaction_hash, 600)
    if receipt["status"] != 1:
        raise RuntimeError(f"Approval transaction {transaction_hash.hex()} failed")
    return receipt


if __name__ == "__main__":
    main()
