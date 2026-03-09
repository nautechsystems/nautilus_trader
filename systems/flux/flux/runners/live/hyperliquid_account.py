from __future__ import annotations

import json
from collections.abc import Callable
from dataclasses import dataclass
import sys
from typing import Any
from urllib.request import ProxyHandler
from urllib.request import Request
from urllib.request import build_opener
from urllib.request import urlopen

_CURRENT_MODULE = sys.modules[__name__]

if __name__ == "flux.runners.live.hyperliquid_account":
    sys.modules["nautilus_trader.flux.runners.live.hyperliquid_account"] = _CURRENT_MODULE
elif __name__ == "nautilus_trader.flux.runners.live.hyperliquid_account":
    sys.modules["flux.runners.live.hyperliquid_account"] = _CURRENT_MODULE

live_pkg = sys.modules.get("flux.runners.live")
if live_pkg is not None:
    setattr(live_pkg, "hyperliquid_account", _CURRENT_MODULE)

compat_live_pkg = sys.modules.get("nautilus_trader.flux.runners.live")
if compat_live_pkg is not None:
    setattr(compat_live_pkg, "hyperliquid_account", _CURRENT_MODULE)


HYPERLIQUID_INFO_URL = "https://api.hyperliquid.xyz/info"
HYPERLIQUID_TESTNET_INFO_URL = "https://api.hyperliquid-testnet.xyz/info"

_USER_ROLE_WRAPPER_KEYS = ("data", "result", "response", "payload")
_USER_ROLE_ADDRESS_KEYS = (
    "master",
    "masterAddress",
    "master_address",
    "parent",
    "parentAddress",
    "parent_address",
    "user",
    "address",
)


class HyperliquidUserResolutionError(RuntimeError):
    """Raised when Hyperliquid effective user resolution cannot safely complete."""


@dataclass(frozen=True)
class ResolvedHyperliquidUser:
    execution_signer: str | None
    account_query_address: str | None
    fee_query_address: str | None
    ws_subscription_address: str | None
    source: str


def _is_address(value: Any) -> bool:
    return isinstance(value, str) and len(value) == 42 and value.startswith("0x")


def _addresses_match(left: str | None, right: str | None) -> bool:
    if left is None or right is None:
        return False
    return left.casefold() == right.casefold()


def _extract_master_address(payload: Any, signer: str) -> str | None:
    if _is_address(payload):
        candidate = str(payload)
        if not _addresses_match(candidate, signer):
            return candidate
        return None

    if isinstance(payload, dict):
        for key in _USER_ROLE_ADDRESS_KEYS:
            candidate = _extract_master_address(payload.get(key), signer)
            if candidate is not None:
                return candidate
        for key in _USER_ROLE_WRAPPER_KEYS:
            candidate = _extract_master_address(payload.get(key), signer)
            if candidate is not None:
                return candidate

    if isinstance(payload, list):
        for item in payload:
            candidate = _extract_master_address(item, signer)
            if candidate is not None:
                return candidate

    return None


def _extract_user_role(payload: Any) -> str | None:
    if not isinstance(payload, dict):
        return None

    role = payload.get("role")
    if isinstance(role, str):
        normalized = role.strip().lower()
        return normalized or None

    for key in _USER_ROLE_WRAPPER_KEYS:
        nested_role = _extract_user_role(payload.get(key))
        if nested_role is not None:
            return nested_role

    return None


def _post_hyperliquid_info(
    *,
    payload: dict[str, Any],
    testnet: bool,
    timeout_secs: int,
    http_proxy_url: str | None,
) -> Any:
    url = HYPERLIQUID_TESTNET_INFO_URL if testnet else HYPERLIQUID_INFO_URL
    request = Request(
        url=url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    if http_proxy_url:
        opener = build_opener(
            ProxyHandler({"http": http_proxy_url, "https": http_proxy_url}),
        )
        with opener.open(request, timeout=timeout_secs) as response:
            return json.loads(response.read().decode("utf-8"))

    with urlopen(request, timeout=timeout_secs) as response:  # noqa: S310
        return json.loads(response.read().decode("utf-8"))


def _resolve_user_role_master_address(
    *,
    execution_signer: str,
    testnet: bool,
    timeout_secs: int,
    http_proxy_url: str | None,
    info_client: Callable[..., Any] | None,
) -> str | None:
    client = info_client or _post_hyperliquid_info
    payload = {"type": "userRole", "user": execution_signer}
    response = client(
        payload=payload,
        testnet=testnet,
        timeout_secs=timeout_secs,
        http_proxy_url=http_proxy_url,
    )
    resolved_address = _extract_master_address(response, execution_signer)
    if _extract_user_role(response) == "agent" and resolved_address is None:
        raise HyperliquidUserResolutionError(
            "Hyperliquid userRole returned agent role without a distinct funded/master "
            f"address for signer {execution_signer}",
        )
    return resolved_address


def resolve_hyperliquid_user(
    *,
    client: Any,
    account_address: str | None,
    vault_address: str | None,
    testnet: bool,
    http_timeout_secs: int,
    http_proxy_url: str | None,
    info_client: Callable[..., Any] | None = None,
) -> ResolvedHyperliquidUser:
    execution_signer: str | None = None
    try:
        execution_signer = client.get_user_address()
    except Exception:
        execution_signer = None

    if vault_address:
        return ResolvedHyperliquidUser(
            execution_signer=execution_signer,
            account_query_address=vault_address,
            fee_query_address=vault_address,
            ws_subscription_address=vault_address,
            source="vault_address",
        )

    if account_address:
        return ResolvedHyperliquidUser(
            execution_signer=execution_signer,
            account_query_address=account_address,
            fee_query_address=account_address,
            ws_subscription_address=account_address,
            source="account_address",
        )

    if execution_signer is None:
        return ResolvedHyperliquidUser(
            execution_signer=None,
            account_query_address=None,
            fee_query_address=None,
            ws_subscription_address=None,
            source="unresolved",
        )

    try:
        master_address = _resolve_user_role_master_address(
            execution_signer=execution_signer,
            testnet=testnet,
            timeout_secs=http_timeout_secs,
            http_proxy_url=http_proxy_url,
            info_client=info_client,
        )
    except HyperliquidUserResolutionError:
        raise
    except Exception as exc:
        raise HyperliquidUserResolutionError(
            "Hyperliquid userRole lookup failed while resolving the effective account "
            f"for signer {execution_signer}",
        ) from exc

    effective_address = master_address or execution_signer
    source = "user_role_master" if master_address else "execution_signer"
    return ResolvedHyperliquidUser(
        execution_signer=execution_signer,
        account_query_address=effective_address,
        fee_query_address=effective_address,
        ws_subscription_address=effective_address,
        source=source,
    )
