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

from importlib.util import module_from_spec
from importlib.util import spec_from_file_location
from pathlib import Path
from unittest.mock import MagicMock
from unittest.mock import call

import pytest


SCRIPT_PATH = (
    Path(__file__).parents[4]
    / "nautilus_trader"
    / "adapters"
    / "polymarket"
    / "scripts"
    / "set_allowances.py"
)
SPEC = spec_from_file_location("polymarket_set_allowances", SCRIPT_PATH)
assert SPEC is not None
assert SPEC.loader is not None
set_allowances = module_from_spec(SPEC)
SPEC.loader.exec_module(set_allowances)


def test_set_allowances_approves_collateral_and_ctf_for_all_current_targets() -> None:
    web3 = MagicMock()
    collateral = MagicMock()
    ctf = MagicMock()
    web3.to_checksum_address.side_effect = lambda address: address
    web3.eth.contract.side_effect = [collateral, ctf]
    web3.eth.get_transaction_count.return_value = 37
    web3.eth.account.sign_transaction.return_value.raw_transaction = b"signed"
    web3.eth.send_raw_transaction.side_effect = range(6)
    web3.eth.wait_for_transaction_receipt.side_effect = [{"status": 1}] * 6

    set_allowances.set_allowances(web3, "private-key", "public-key")

    expected_targets = (
        "0xE111180000d2663C0091e4f400237545B87B996B",
        "0xe2222d279d744050d28e00520010520000310F59",
        "0xadA2005600Dec949baf300f4C6120000bDB6eAab",
    )
    expected_contract_calls = [
        call(
            address="0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB",
            abi=set_allowances.ERC20_APPROVE_ABI,
        ),
        call(
            address="0x4D97DCd97eC945f40cF65F87097ACe5EA0476045",
            abi=set_allowances.ERC1155_SET_APPROVAL_ABI,
        ),
    ]
    expected_collateral_calls = [
        call(target, set_allowances.MAX_UINT256) for target in expected_targets
    ]
    expected_ctf_calls = [call(target, True) for target in expected_targets]
    expected_collateral_transactions = [
        call({"chainId": 137, "from": "public-key", "nonce": nonce}) for nonce in (37, 39, 41)
    ]
    expected_ctf_transactions = [
        call({"chainId": 137, "from": "public-key", "nonce": nonce}) for nonce in (38, 40, 42)
    ]

    assert web3.eth.contract.call_args_list == expected_contract_calls
    web3.eth.get_transaction_count.assert_called_once_with("public-key", "pending")
    assert set_allowances.DEFAULT_POLYGON_RPC_URL == "https://polygon.drpc.org"
    assert collateral.functions.approve.call_args_list == expected_collateral_calls
    assert ctf.functions.setApprovalForAll.call_args_list == expected_ctf_calls
    assert (
        collateral.functions.approve.return_value.build_transaction.call_args_list
        == expected_collateral_transactions
    )
    assert (
        ctf.functions.setApprovalForAll.return_value.build_transaction.call_args_list
        == expected_ctf_transactions
    )


@pytest.mark.parametrize("failed_receipt_index", range(6))
def test_set_allowances_stops_after_any_failed_receipt(failed_receipt_index: int) -> None:
    web3 = MagicMock()
    collateral = MagicMock()
    ctf = MagicMock()
    web3.to_checksum_address.side_effect = lambda address: address
    web3.eth.contract.side_effect = [collateral, ctf]
    web3.eth.get_transaction_count.return_value = 0
    web3.eth.account.sign_transaction.return_value.raw_transaction = b"signed"
    web3.eth.send_raw_transaction.return_value = b"failed"
    web3.eth.wait_for_transaction_receipt.side_effect = [
        *[{"status": 1}] * failed_receipt_index,
        {"status": 0},
    ]

    with pytest.raises(RuntimeError, match="Approval transaction 6661696c6564 failed"):
        set_allowances.set_allowances(web3, "private-key", "public-key")

    assert web3.eth.send_raw_transaction.call_count == failed_receipt_index + 1
    assert web3.eth.wait_for_transaction_receipt.call_count == failed_receipt_index + 1
