from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from flux.common.account_projection import ProfileAccountProviderBinding
from flux.common.account_scopes import AccountScopeConfig
from flux.common.account_scopes import decode_account_scopes
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.strategies.makerv4.reference_balances import IbkrReferenceBalanceSnapshotProviderConfig
from flux.strategies.makerv4.reference_balances import get_cached_ibkr_reference_balance_provider
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _build_ibkr_account_provider(
    *,
    scope_config: AccountScopeConfig,
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> Any | None:
    _ = (account_scope_id, source_strategy_ids)
    dockerized_gateway_cfg = scope_config.dockerized_gateway
    dockerized_gateway = None
    if isinstance(dockerized_gateway_cfg, DockerizedIBGatewayConfig):
        dockerized_gateway = dockerized_gateway_cfg
    elif isinstance(dockerized_gateway_cfg, Mapping):
        dockerized_gateway = DockerizedIBGatewayConfig(**dockerized_gateway_cfg)
    elif dockerized_gateway_cfg is not None:
        raise ValueError("`node.venues.IBKR.dockerized_gateway` must be a TOML table")

    return get_cached_ibkr_reference_balance_provider(
        IbkrReferenceBalanceSnapshotProviderConfig(
            ibg_host=scope_config.ibg_host or "127.0.0.1",
            ibg_port=None if dockerized_gateway is not None else scope_config.ibg_port,
            ibg_client_id=(
                1 if scope_config.ibg_client_id is None else scope_config.ibg_client_id
            ),
            dockerized_gateway=dockerized_gateway,
            connection_timeout=300,
            request_timeout_secs=60,
            account_id=scope_config.account_id,
        ),
    )


def build_account_projection_provider(
    *,
    scope_config: AccountScopeConfig,
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> Any | None:
    provider_id = scope_config.provider.strip().lower()
    if provider_id == "ibkr":
        return _build_ibkr_account_provider(
            scope_config=scope_config,
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


def _decode_account_scope_map(
    config: Mapping[str, Any],
) -> tuple[tuple[AccountScopeConfig, ...], dict[str, AccountScopeConfig]]:
    decoded = decode_account_scopes(config.get("account_scopes") or [])
    by_id: dict[str, AccountScopeConfig] = {}
    for scope_config in decoded:
        if scope_config.scope_id in by_id:
            raise ValueError(f"duplicate account scope_id {scope_config.scope_id!r}")
        by_id[scope_config.scope_id] = scope_config
    return decoded, by_id


def build_profile_account_provider_bindings(
    *,
    config: Mapping[str, Any],
) -> tuple[ProfileAccountProviderBinding, ...]:
    contracts = decode_strategy_contracts(config.get("strategy_contracts") or [])
    scope_configs, scope_config_by_id = _decode_account_scope_map(config)
    grouped_strategy_ids: dict[str, list[str]] = {}
    for contract in contracts:
        for account_scope_id in _scope_candidates(contract):
            strategy_ids = grouped_strategy_ids.setdefault(account_scope_id, [])
            if contract.strategy_id not in strategy_ids:
                strategy_ids.append(contract.strategy_id)

    missing_scope_ids = [
        account_scope_id
        for account_scope_id in grouped_strategy_ids
        if account_scope_id not in scope_config_by_id
    ]
    if missing_scope_ids:
        raise ValueError(
            "missing shared account scope config for "
            + ", ".join(sorted(missing_scope_ids)),
        )

    bindings: list[ProfileAccountProviderBinding] = []
    for scope_config in scope_configs:
        strategy_ids = grouped_strategy_ids.get(scope_config.scope_id)
        if not strategy_ids:
            continue
        provider = build_account_projection_provider(
            scope_config=scope_config,
            account_scope_id=scope_config.scope_id,
            source_strategy_ids=tuple(strategy_ids),
        )
        bindings.append(
            ProfileAccountProviderBinding(
                account_scope_id=scope_config.scope_id,
                source_strategy_ids=tuple(strategy_ids),
                provider=provider,
            ),
        )
    return tuple(bindings)

__all__ = (
    "build_account_projection_provider",
    "build_profile_account_provider_bindings",
)
