from __future__ import annotations

import json
from collections.abc import Iterable
from collections.abc import Mapping
from typing import Any

from nautilus_trader.adapters.interactive_brokers.common import IBContract


type IBContractSpec = IBContract | Mapping[str, Any]


def ib_contract_spec_to_dict(contract: IBContractSpec) -> dict[str, Any]:
    if isinstance(contract, Mapping):
        return dict(contract)

    if isinstance(contract, IBContract):
        return json.loads(contract.json().decode())

    raise TypeError(
        f"Expected IBContract or mapping, received {type(contract).__name__}",
    )


def ib_contract_specs_to_dicts(
    contracts: Iterable[IBContractSpec] | None,
) -> list[dict[str, Any]] | None:
    if contracts is None:
        return None

    contracts = list(contracts)
    if not contracts:
        return None

    return [ib_contract_spec_to_dict(contract) for contract in contracts]
