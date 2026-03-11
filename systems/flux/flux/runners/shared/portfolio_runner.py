from __future__ import annotations

import inspect
import logging
import signal
import sys
import time
from typing import Any

from flux.common.account_projection import build_profile_account_snapshot
from flux.common.account_projection import encode_profile_account_snapshot
from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import aggregate_components
from flux.common.portfolio_inventory import decode_component
from flux.common.portfolio_inventory import encode_portfolio_inventory
from flux.common.portfolio_snapshot import build_balance_rows_by_strategy
from flux.common.portfolio_snapshot import build_portfolio_snapshot
from flux.common.portfolio_snapshot import encode_portfolio_snapshot
from flux.runners.shared.bootstrap import build_redis_client
from flux.runners.shared.bootstrap import table
from flux.runners.shared.strategy_set import StrategySetDescriptor


if __name__ == "flux.runners.shared.portfolio_runner":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared.portfolio_runner", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared.portfolio_runner":
    sys.modules.setdefault("flux.runners.shared.portfolio_runner", sys.modules[__name__])


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def parse_strategy_ids(api_cfg: dict[str, Any], *, descriptor: StrategySetDescriptor) -> list[str]:
    raw = api_cfg.get(descriptor.strategy_ids_field) or []
    if not isinstance(raw, list):
        raise ValueError(f"`api.{descriptor.strategy_ids_field}` must be a TOML array")
    out: list[str] = []
    seen: set[str] = set()
    for value in raw:
        text = _optional_text(value)
        if not text or text in seen:
            continue
        seen.add(text)
        out.append(text)
    if not out:
        raise ValueError(f"`api.{descriptor.strategy_ids_field}` must be non-empty")
    return out


def parse_required_strategy_ids(
    api_cfg: dict[str, Any],
    *,
    descriptor: StrategySetDescriptor,
    fallback: list[str],
) -> list[str]:
    raw = api_cfg.get(descriptor.required_strategy_ids_field) or []
    if not raw:
        return list(fallback)
    if not isinstance(raw, list):
        raise ValueError(f"`api.{descriptor.required_strategy_ids_field}` must be a TOML array")
    out: list[str] = []
    seen: set[str] = set()
    allowlist = set(fallback)
    for value in raw:
        text = _optional_text(value)
        if not text or text in seen:
            continue
        if text not in allowlist:
            raise ValueError(
                f"required {descriptor.profile.title()} strategy not in allowlist: {text}",
            )
        seen.add(text)
        out.append(text)
    return out or list(fallback)


def portfolio_base_assets(config: dict[str, Any], *, fallback: list[str]) -> list[str]:
    contracts = config.get("contracts") or []
    out: list[str] = []
    seen: set[str] = set()
    if isinstance(contracts, list):
        for item in contracts:
            if not isinstance(item, dict):
                continue
            symbol = _optional_text(item.get("symbol")) or ""
            base = symbol.split("/", maxsplit=1)[0].strip().upper()
            if not base or base in seen:
                continue
            seen.add(base)
            out.append(base)
    return out or list(fallback)


class StrategySetPortfolioAggregator:
    def __init__(
        self,
        *,
        config: dict[str, Any],
        mode: str,
        logger: logging.Logger,
        descriptor: StrategySetDescriptor,
    ) -> None:
        flux = table(config, "flux")
        redis_cfg = table(config, "redis")
        api_cfg = table(config, "api")
        portfolio_cfg = table(config, "portfolio")

        self._descriptor = descriptor
        self._namespace = str(flux.get("namespace", "flux"))
        self._schema_version = str(flux.get("schema_version", "v1"))
        self._mode = mode
        self._portfolio_id = _optional_text(portfolio_cfg.get("portfolio_id")) or descriptor.default_portfolio_id
        self._stale_after_ms = int(
            portfolio_cfg.get(
                "inventory_stale_after_ms",
                DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
            ),
        )
        self._aggregation_mode = str(
            portfolio_cfg.get("inventory_aggregation_mode", "strict"),
        ).strip().lower() or "strict"
        self._strategy_ids = parse_strategy_ids(api_cfg, descriptor=descriptor)
        self._required_strategy_ids = set(
            parse_required_strategy_ids(
                api_cfg,
                descriptor=descriptor,
                fallback=self._strategy_ids,
            ),
        )
        self._base_assets = portfolio_base_assets(config, fallback=["PLUME"])
        self._redis = build_redis_client(redis_cfg)
        self._log = logger
        self._running = True
        self._profile_account_bindings = ()
        self.account_scope_ids: list[str] = []

    def stop(self, *_args: Any) -> None:
        self._running = False

    def _component_key(self, *, strategy_id: str, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory_component(
            strategy_id=strategy_id,
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _aggregate_key(self, *, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory(
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _aggregate_channel(self, *, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory_channel(
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _balances_snapshot_key(self, *, strategy_id: str) -> str:
        return FluxRedisKeys(
            strategy_id=strategy_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        ).balances_snapshot()

    def _snapshot_key(self) -> str:
        return FluxRedisKeys.portfolio_snapshot(
            portfolio_id=self._portfolio_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _snapshot_channel(self) -> str:
        return FluxRedisKeys.portfolio_snapshot_channel(
            portfolio_id=self._portfolio_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _profile_account_projection_key(self, *, account_scope_id: str) -> str:
        return FluxRedisKeys.profile_account_projection(
            profile_id=self._descriptor.profile,
            account_scope_id=account_scope_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _profile_account_projection_channel(self, *, account_scope_id: str) -> str:
        return FluxRedisKeys.profile_account_projection_channel(
            profile_id=self._descriptor.profile,
            account_scope_id=account_scope_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _publish_profile_account_projections(self, *, now_ms_value: int) -> None:
        for binding in getattr(self, "_profile_account_bindings", ()):
            provider = binding.provider
            if provider is None:
                continue
            refresh = getattr(provider, "refresh", None)
            if callable(refresh):
                try:
                    refresh()
                except Exception as exc:
                    self._log.warning(
                        "Failed to refresh account projection scope %s: %s",
                        binding.account_scope_id,
                        exc,
                    )
            snapshot = build_profile_account_snapshot(
                profile_id=self._descriptor.profile,
                bindings=[binding],
                ts_ms=now_ms_value,
            )
            if not snapshot["rows"]:
                continue
            encoded = encode_profile_account_snapshot(snapshot)
            key = self._profile_account_projection_key(account_scope_id=binding.account_scope_id)
            previous = self._redis.get(key)
            self._redis.set(key, encoded)
            if previous != encoded.encode():
                self._redis.publish(
                    self._profile_account_projection_channel(account_scope_id=binding.account_scope_id),
                    encoded,
                )

    def recompute_once(self) -> None:
        now_ms_value = int(time.time() * 1000)
        self._publish_profile_account_projections(now_ms_value=now_ms_value)
        balances_pipeline = self._redis.pipeline(transaction=False)
        for strategy_id in self._strategy_ids:
            balances_pipeline.get(self._balances_snapshot_key(strategy_id=strategy_id))
        raw_balance_snapshots = balances_pipeline.execute()
        balance_rows_by_strategy = build_balance_rows_by_strategy(
            raw_snapshots_by_strategy=dict(
                zip(
                    self._strategy_ids,
                    raw_balance_snapshots,
                    strict=True,
                ),
            ),
        )
        for base_currency in self._base_assets:
            pipeline = self._redis.pipeline(transaction=False)
            for strategy_id in self._strategy_ids:
                pipeline.get(self._component_key(strategy_id=strategy_id, base_currency=base_currency))
            raw_components = pipeline.execute()
            components = {
                strategy_id: decode_component(raw)
                for strategy_id, raw in zip(self._strategy_ids, raw_components, strict=True)
            }
            payload = aggregate_components(
                portfolio_id=self._portfolio_id,
                base_currency=base_currency,
                components=components,
                required_strategy_ids=self._required_strategy_ids,
                now_ms_value=now_ms_value,
                stale_after_ms=self._stale_after_ms,
                aggregation_mode=getattr(self, "_aggregation_mode", "strict"),
            )
            encoded = encode_portfolio_inventory(payload)
            key = self._aggregate_key(base_currency=base_currency)
            previous = self._redis.get(key)
            self._redis.set(key, encoded)
            if previous != encoded.encode():
                self._redis.publish(self._aggregate_channel(base_currency=base_currency), encoded)
            snapshot_writer = getattr(self, "_snapshot_writer", None)
            if snapshot_writer is not None:
                snapshot_writer.maybe_persist(payload=payload, ts_ms=now_ms_value)

            snapshot = build_portfolio_snapshot(
                portfolio_id=self._portfolio_id,
                base_currency=base_currency,
                inventory_components=components,
                balance_rows_by_strategy=balance_rows_by_strategy,
                required_strategy_ids=self._required_strategy_ids,
                now_ms_value=now_ms_value,
                stale_after_ms=self._stale_after_ms,
                aggregation_mode=getattr(self, "_aggregation_mode", "strict"),
                inventory_payload=payload,
            )
            encoded_snapshot = encode_portfolio_snapshot(snapshot)
            snapshot_key = self._snapshot_key()
            previous_snapshot = self._redis.get(snapshot_key)
            self._redis.set(snapshot_key, encoded_snapshot)
            if previous_snapshot != encoded_snapshot.encode():
                self._redis.publish(self._snapshot_channel(), encoded_snapshot)

    def _stop_profile_account_providers(self) -> None:
        for binding in getattr(self, "_profile_account_bindings", ()):
            provider_stop = getattr(binding.provider, "stop", None)
            if callable(provider_stop):
                try:
                    provider_stop()
                except Exception as exc:
                    self._log.warning(
                        "Failed to stop account projection scope %s cleanly: %s",
                        binding.account_scope_id,
                        exc,
                    )

    def _disconnect_redis_connection_pool(self) -> None:
        connection_pool = getattr(self._redis, "connection_pool", None)
        disconnect = getattr(connection_pool, "disconnect", None)
        if not callable(disconnect):
            return
        try:
            parameter_names = tuple(inspect.signature(disconnect).parameters)
            if "in_use_connections" in parameter_names:
                disconnect(in_use_connections=False)
            elif "inuse_connections" in parameter_names:
                disconnect(inuse_connections=False)
            else:
                disconnect()
        except Exception as exc:
            self._log.warning("Failed to disconnect redis connection pool cleanly: %s", exc)

    def _shutdown(self) -> None:
        self._stop_profile_account_providers()
        snapshot_writer = getattr(self, "_snapshot_writer", None)
        if snapshot_writer is not None:
            try:
                snapshot_writer.close()
            except Exception as exc:
                self._log.warning("Failed to close portfolio inventory snapshot writer cleanly: %s", exc)

        close = getattr(self._redis, "close", None)
        if callable(close):
            try:
                close()
            except Exception as exc:
                self._log.warning("Failed to close redis client cleanly: %s", exc)
        self._disconnect_redis_connection_pool()

    def run(self) -> None:
        signal.signal(signal.SIGINT, self.stop)
        signal.signal(signal.SIGTERM, self.stop)
        self._log.info(
            "%s portfolio aggregator started portfolio_id=%s mode=%s bases=%s strategies=%s",
            self._descriptor.profile,
            self._portfolio_id,
            self._mode,
            self._base_assets,
            self._strategy_ids,
        )
        try:
            while self._running:
                self.recompute_once()
                time.sleep(0.25)
        finally:
            self._shutdown()
