# ruff: noqa

import os

from web3 import Web3
from web3.constants import MAX_INT
from web3.middleware import geth_poa_middleware

# Before running this script you will need the following:
# - Install the web3 Python package (pip install -U web3==5.28)
# - A Polygon wallet funded with some MATIC

rpc_url = "https://polygon-rpc.com/"
priv_key = os.environ["POLYGON_PRIVATE_KEY"]
pub_key = os.environ["POLYGON_PUBLIC_KEY"]

chain_id = 137

erc20_approve = """[{"constant": false,"inputs": [{"name": "_spender","type": "address" },{ "name": "_value", "type": "uint256" }],"name": "approve","outputs": [{ "name": "", "type": "bool" }],"payable": false,"stateMutability": "nonpayable","type": "function"}]"""
erc1155_set_approval = """[{"inputs": [{ "internalType": "address", "name": "operator", "type": "address" },{ "internalType": "bool", "name": "approved", "type": "bool" }],"name": "setApprovalForAll","outputs": [],"stateMutability": "nonpayable","type": "function"}]"""

# Polymarket USDC token contract
usdc_address = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"

# Polymarket CTF contract
ctf_address = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"

web3 = Web3(Web3.HTTPProvider(rpc_url))
web3.middleware_onion.inject(geth_poa_middleware, layer=0)

nonce = web3.eth.getTransactionCount(pub_key)

usdc = web3.eth.contract(address=usdc_address, abi=erc20_approve)
ctf = web3.eth.contract(address=ctf_address, abi=erc1155_set_approval)

# CTF Exchange
raw_usdc_approve_txn = usdc.functions.approve(
    "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E",
    int(MAX_INT, 0),
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(
    raw_usdc_approve_txn,
    private_key=priv_key,
)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.rawTransaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print(usdc_approve_tx_receipt)

nonce = web3.eth.getTransactionCount(pub_key)

raw_ctf_approval_txn = ctf.functions.setApprovalForAll(
    "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E",
    True,
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(
    raw_ctf_approval_txn,
    private_key=priv_key,
)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.rawTransaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print(ctf_approval_tx_receipt)

nonce = web3.eth.getTransactionCount(pub_key)

# Neg Risk CTF Exchange
raw_usdc_approve_txn = usdc.functions.approve(
    "0xC5d563A36AE78145C45a50134d48A1215220f80a",
    int(MAX_INT, 0),
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(
    raw_usdc_approve_txn,
    private_key=priv_key,
)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.rawTransaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print(usdc_approve_tx_receipt)

nonce = web3.eth.getTransactionCount(pub_key)

raw_ctf_approval_txn = ctf.functions.setApprovalForAll(
    "0xC5d563A36AE78145C45a50134d48A1215220f80a",
    True,
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(
    raw_ctf_approval_txn,
    private_key=priv_key,
)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.rawTransaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print(ctf_approval_tx_receipt)

nonce = web3.eth.getTransactionCount(pub_key)

# Neg Risk Adapter
raw_usdc_approve_txn = usdc.functions.approve(
    "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296",
    int(MAX_INT, 0),
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(
    raw_usdc_approve_txn,
    private_key=priv_key,
)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.rawTransaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print(usdc_approve_tx_receipt)

nonce = web3.eth.getTransactionCount(pub_key)

raw_ctf_approval_txn = ctf.functions.setApprovalForAll(
    "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296",
    True,
).buildTransaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(
    raw_ctf_approval_txn,
    private_key=priv_key,
)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.rawTransaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print(ctf_approval_tx_receipt)
