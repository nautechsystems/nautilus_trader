from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from flux.common.account_projection import ProfileAccountProviderBinding
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.strategies.makerv4.reference_balances import IbkrReferenceBalanceSnapshotProviderConfig
from flux.strategies.makerv4.reference_balances import get_cached_ibkr_reference_balance_provider
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _mapping(value: Any) -> Mapping[str, Any]:
    if isinstance(value, Mapping):
        return value
    return {}


def _build_ibkr_account_provider(
    *,
    config: Mapping[str, Any],
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> Any | None:
    node_cfg = _mapping(config.get("node"))
    venue_entries = _mapping(node_cfg.get("venues"))
    ibkr_cfg = _mapping(venue_entries.get("IBKR"))
    if not ibkr_cfg:
        return None
    adapter_id = (_optional_text(ibkr_cfg.get("adapter")) or "ibkr").lower()
    if adapter_id not in {"ibkr", "interactive_brokers"}:
        return None

    dockerized_gateway_cfg = ibkr_cfg.get("dockerized_gateway")
    dockerized_gateway = None
    if isinstance(dockerized_gateway_cfg, DockerizedIBGatewayConfig):
        dockerized_gateway = dockerized_gateway_cfg
    elif isinstance(dockerized_gateway_cfg, Mapping):
        dockerized_gateway = DockerizedIBGatewayConfig(**dockerized_gateway_cfg)
    elif dockerized_gateway_cfg is not None:
        raise ValueError("`node.venues.IBKR.dockerized_gateway` must be a TOML table")

    return get_cached_ibkr_reference_balance_provider(
        IbkrReferenceBalanceSnapshotProviderConfig(
            ibg_host=_optional_text(ibkr_cfg.get("ibg_host")) or "127.0.0.1",
            ibg_port=None if ibkr_cfg.get("ibg_port") is None else int(ibkr_cfg.get("ibg_port")),
            ibg_client_id=int(ibkr_cfg.get("ibg_client_id", 1)),
            dockerized_gateway=dockerized_gateway,
            connection_timeout=int(ibkr_cfg.get("connection_timeout", 300)),
            request_timeout_secs=int(ibkr_cfg.get("request_timeout_secs", 60)),
            account_id=_optional_text(ibkr_cfg.get("account_id")),
        ),
    )


def build_account_projection_provider(
    *,
    config: Mapping[str, Any],
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> Any | None:
    scope_prefix = str(account_scope_id).split(".", maxsplit=1)[0].strip().lower()
    if scope_prefix == "ibkr":
        return _build_ibkr_account_provider(
            config=config,
            account_scope_id=account_scope_id,
            source_strategy_ids=source_strategy_ids,
        )
    return None


def _scope_candidates(contract: Any) -> tuple[str, ...]:
    candidates = (
        _optional_text(getattr(contract, "execution_account_scope_id", None)),
        _optional_text(getattr(contract, "reference_account_scope_id", None)),
        _optional_text(getattr(contract, "hedge_account_scope_id", None)),
    )
    return tuple(scope_id for scope_id in candidates if scope_id is not None)


def build_profile_account_provider_bindings(
    *,
    config: Mapping[str, Any],
) -> tuple[ProfileAccountProviderBinding, ...]:
    contracts = decode_strategy_contracts(config.get("strategy_contracts") or [])
    grouped_strategy_ids: dict[str, list[str]] = {}
    for contract in contracts:
        for account_scope_id in _scope_candidates(contract):
            strategy_ids = grouped_strategy_ids.setdefault(account_scope_id, [])
            if contract.strategy_id not in strategy_ids:
                strategy_ids.append(contract.strategy_id)

    bindings: list[ProfileAccountProviderBinding] = []
    for account_scope_id, strategy_ids in grouped_strategy_ids.items():
        provider = build_account_projection_provider(
            config=config,
            account_scope_id=account_scope_id,
            source_strategy_ids=tuple(strategy_ids),
        )
        bindings.append(
            ProfileAccountProviderBinding(
                account_scope_id=account_scope_id,
                source_strategy_ids=tuple(strategy_ids),
                provider=provider,
            ),
        )
    return tuple(bindings)

__all__ = (
    "build_account_projection_provider",
    "build_profile_account_provider_bindings",
)
