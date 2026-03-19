from __future__ import annotations

import asyncio
import copy
import logging
import os
import time
from collections.abc import Mapping
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation
from typing import Any

from flux.api._payloads_balances import build_balances_rows
from flux.api._payloads_common import canonical_naming_fields
from flux.common.account_projection import ProfileAccountProviderBinding
from flux.common.account_scopes import AccountScopeConfig
from flux.common.account_scopes import decode_account_scopes
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.runners.live.hyperliquid_account import _post_hyperliquid_info
from flux.runners.live.hyperliquid_account import resolve_hyperliquid_user
from flux.strategies.makerv4.reference_balances import IbkrReferenceBalanceSnapshotProviderConfig
from flux.strategies.makerv4.reference_balances import get_cached_ibkr_reference_balance_provider
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily
from nautilus_trader.adapters.binance.common.urls import get_private_http_base_url
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.hyperliquid.factories import get_cached_hyperliquid_http_client
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.common.component import LiveClock


_ACCOUNT_PROJECTION_LOG = logging.getLogger("nautilus-equities-account-projection")


@dataclass(frozen=True)
class HyperliquidAccountProjectionProviderConfig:
    private_key: str | None = None
    account_address: str | None = None
    vault_address: str | None = None
    dex: str | None = None
    testnet: bool = False
    http_timeout_secs: int = 10
    http_proxy_url: str | None = None
    refresh_interval_secs: float = 15.0


@dataclass(frozen=True)
class BinanceFuturesAccountProjectionProviderConfig:
    api_key: str
    api_secret: str
    account_type: BinanceAccountType = BinanceAccountType.USDT_FUTURES
    private_api_family: BinancePrivateApiFamily = BinancePrivateApiFamily.AUTO
    environment: BinanceEnvironment = BinanceEnvironment.LIVE
    base_url_http: str | None = None
    recv_window_ms: int = 5000
    http_proxy_url: str | None = None
    refresh_interval_secs: float = 15.0


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _env_value(env_name: str | None) -> str | None:
    if env_name is None:
        return None
    text = env_name.strip()
    if not text:
        return None
    return _optional_text(os.environ.get(text))


def _safe_float(value: Any) -> float | None:
    try:
        out = float(value)
    except (TypeError, ValueError):
        return None
    return out if out == out and out not in (float("inf"), float("-inf")) else None


def _money_display(value: float) -> str:
    return f"{'-$' if value < 0 else '$'}{abs(value):.2f}"


def _field_value(value: Any, *field_names: str) -> Any:
    for field_name in field_names:
        if isinstance(value, Mapping):
            candidate = value.get(field_name)
            if candidate is not None:
                return candidate
        candidate = getattr(value, field_name, None)
        if candidate is not None:
            return candidate
    return None


def _safe_decimal(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        out = Decimal(str(value))
    except (InvalidOperation, TypeError, ValueError):
        return None
    return out if out.is_finite() else None


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    return text or "0"


def _locked_balance_text(total: Any, free: Any) -> str:
    total_decimal = _safe_decimal(total)
    free_decimal = _safe_decimal(free)
    if total_decimal is None or free_decimal is None:
        return "0"
    locked = total_decimal - free_decimal
    if locked < 0:
        locked = Decimal("0")
    return _decimal_text(locked)


def _extract_hyperliquid_account_totals(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, Mapping):
        return {}

    margin_summary = payload.get("marginSummary")
    if not isinstance(margin_summary, Mapping):
        margin_summary = payload.get("crossMarginSummary")
    if not isinstance(margin_summary, Mapping):
        margin_summary = {}

    account_value = _safe_float(
        margin_summary.get("accountValue") if isinstance(margin_summary, Mapping) else None,
    )
    if account_value is None:
        account_value = _safe_float(payload.get("accountValue"))

    withdrawable = _safe_float(payload.get("withdrawable"))

    totals: dict[str, Any] = {}
    if account_value is not None:
        totals["account_equity_raw"] = account_value
        totals["account_equity_display"] = _money_display(account_value)
    if withdrawable is not None:
        totals["withdrawable_raw"] = withdrawable
        totals["withdrawable_display"] = _money_display(withdrawable)
    return totals


def _extract_hyperliquid_perp_usdc_balance(payload: Any) -> dict[str, str] | None:
    if not isinstance(payload, Mapping):
        return None

    margin_summary = payload.get("crossMarginSummary")
    if not isinstance(margin_summary, Mapping):
        margin_summary = payload.get("marginSummary")
    if not isinstance(margin_summary, Mapping):
        margin_summary = {}

    total = _safe_decimal(
        margin_summary.get("totalRawUsd")
        if isinstance(margin_summary, Mapping)
        else None,
    )
    if total is None:
        total = _safe_decimal(
            margin_summary.get("accountValue")
            if isinstance(margin_summary, Mapping)
            else None,
        )
    if total is None:
        total = _safe_decimal(payload.get("accountValue"))

    free = _safe_decimal(payload.get("withdrawable"))
    if free is None and isinstance(margin_summary, Mapping):
        free = _safe_decimal(margin_summary.get("withdrawable"))

    if total is None and free is None:
        return None
    if total is None:
        total = free
    if free is None:
        free = total
    if total is None or free is None:
        return None

    total = max(total, Decimal("0"))
    free = max(free, Decimal("0"))
    if free > total:
        total = free
    locked = max(total - free, Decimal("0"))
    return {
        "currency": "USDC",
        "free": _decimal_text(free),
        "locked": _decimal_text(locked),
        "total": _decimal_text(total),
    }


def _extract_hyperliquid_spot_balances(payload: Any) -> list[dict[str, Any]]:
    if not isinstance(payload, Mapping):
        return []
    raw_balances = payload.get("balances")
    if not isinstance(raw_balances, list):
        return []

    balances: list[dict[str, Any]] = []
    for raw_balance in raw_balances:
        if not isinstance(raw_balance, Mapping):
            continue
        asset = _optional_text(
            raw_balance.get("coin")
            or raw_balance.get("currency")
            or raw_balance.get("asset"),
        )
        total = _optional_text(raw_balance.get("total") or raw_balance.get("free"))
        locked = _optional_text(raw_balance.get("hold") or raw_balance.get("locked")) or "0"
        if asset is None or total is None:
            continue
        balances.append(
            {
                "currency": asset,
                "free": total,
                "locked": locked,
                "total": total,
            },
        )
    return balances


def _extract_hyperliquid_cash_balances(
    *,
    clearinghouse_payload: Any,
    spot_payload: Any,
) -> list[dict[str, Any]]:
    balances: list[dict[str, Any]] = []
    spot_balances = _extract_hyperliquid_spot_balances(spot_payload)
    preferred_usdc_balance = next(
        (
            balance
            for balance in spot_balances
            if _optional_text(balance.get("currency")) == "USDC"
        ),
        None,
    )
    if preferred_usdc_balance is None:
        preferred_usdc_balance = _extract_hyperliquid_perp_usdc_balance(clearinghouse_payload)
    if preferred_usdc_balance is not None:
        balances.append(preferred_usdc_balance)

    for balance in spot_balances:
        asset = _optional_text(balance.get("currency"))
        if preferred_usdc_balance is not None and asset == "USDC":
            continue
        balances.append(balance)
    return balances


def _parse_binance_account_type(value: str | None) -> BinanceAccountType:
    text = _optional_text(value)
    if text is None:
        raise ValueError("`account_type` is required for Binance shared account scopes")
    try:
        return BinanceAccountType(text)
    except ValueError as exc:
        raise ValueError(f"unsupported Binance account_type {text!r}") from exc


def _parse_binance_private_api_family(value: str | None) -> BinancePrivateApiFamily:
    text = _optional_text(value)
    if text is None:
        return BinancePrivateApiFamily.AUTO
    try:
        return BinancePrivateApiFamily(text)
    except ValueError as exc:
        raise ValueError(f"unsupported Binance private_api_family {text!r}") from exc


def _extract_binance_account_totals(payload: Any) -> dict[str, Any]:
    totals: dict[str, Any] = {}
    account_equity = _safe_float(_field_value(payload, "totalMarginBalance"))
    withdrawable = _safe_float(
        _field_value(payload, "maxWithdrawAmount", "availableBalance"),
    )
    if account_equity is not None:
        totals["account_equity_raw"] = account_equity
        totals["account_equity_display"] = _money_display(account_equity)
    if withdrawable is not None:
        totals["withdrawable_raw"] = withdrawable
        totals["withdrawable_display"] = _money_display(withdrawable)
    return totals


def _extract_binance_futures_balances(payload: Any) -> list[dict[str, Any]]:
    raw_balances = _field_value(payload, "assets")
    if not isinstance(raw_balances, list):
        return []

    balances: list[dict[str, Any]] = []
    for raw_balance in raw_balances:
        asset = _optional_text(_field_value(raw_balance, "asset", "currency", "coin"))
        total = _optional_text(
            _field_value(raw_balance, "walletBalance", "marginBalance", "total", "free"),
        )
        free = _optional_text(_field_value(raw_balance, "availableBalance", "free")) or total
        if asset is None or total is None or free is None:
            continue
        balances.append(
            {
                "currency": asset,
                "free": free,
                "locked": _locked_balance_text(total, free),
                "total": total,
            },
        )
    return balances


def _binance_perp_base_asset(symbol: str) -> str:
    text = symbol.strip().upper()
    for suffix in ("USDT", "USDC", "FDUSD", "USD"):
        if text.endswith(suffix) and len(text) > len(suffix):
            return text[: -len(suffix)]
    return text


def _extract_binance_futures_positions(payload: Any, *, account_id: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not isinstance(payload, list):
        return rows

    for raw_position in payload:
        symbol = _optional_text(_field_value(raw_position, "symbol"))
        position_amt = _safe_decimal(_field_value(raw_position, "positionAmt"))
        if symbol is None or position_amt is None or position_amt == 0:
            continue
        base_asset = _binance_perp_base_asset(symbol)

        rows.append(
            {
                "account_id": account_id,
                "account": account_id,
                "exchange": "binance_perp",
                "kind": "position",
                "asset": base_asset,
                "coin": base_asset,
                "base": base_asset,
                "instrument_id": f"{symbol}-PERP.BINANCE_PERP",
                "signed_qty": _decimal_text(position_amt),
                "quantity": _decimal_text(abs(position_amt)),
                "free": _decimal_text(position_amt),
                "total": _decimal_text(position_amt),
                "entry_price": _field_value(raw_position, "entryPrice"),
                "mark": _field_value(raw_position, "markPrice"),
                "mark_raw": _field_value(raw_position, "markPrice"),
                "unrealized_pnl": _field_value(raw_position, "unRealizedProfit"),
                "market_type": "perp",
                "product_type": "perp",
                "ts_ms": _field_value(raw_position, "updateTime"),
            },
        )
    return rows


class HyperliquidAccountProjectionProvider:
    def __init__(self, config: HyperliquidAccountProjectionProviderConfig) -> None:
        self._config = config
        self._client = get_cached_hyperliquid_http_client(
            private_key=config.private_key,
            account_address=config.account_address,
            vault_address=config.vault_address,
            timeout_secs=config.http_timeout_secs,
            testnet=config.testnet,
            proxy_url=config.http_proxy_url,
            dex=config.dex,
        )
        self._resolved_user: Any | None = None
        self._latest_snapshot: dict[str, Any] | None = None
        self._last_refresh_monotonic = 0.0
        self._client_identity_initialized = False

    def stop(self) -> None:
        return None

    def snapshot(self) -> dict[str, Any] | None:
        if self._latest_snapshot is None:
            return None
        return copy.deepcopy(self._latest_snapshot)

    def refresh(self) -> dict[str, Any] | None:
        now = time.monotonic()
        if (
            self._latest_snapshot is not None
            and (now - self._last_refresh_monotonic) < self._config.refresh_interval_secs
        ):
            return self.snapshot()

        try:
            self._latest_snapshot = asyncio.run(self._fetch_snapshot())
            self._last_refresh_monotonic = time.monotonic()
        except Exception as exc:
            _ACCOUNT_PROJECTION_LOG.warning(
                "Hyperliquid shared-account refresh failed: %s",
                exc,
            )
        return self.snapshot()

    def _request_kwargs(self) -> dict[str, Any]:
        if self._resolved_user is None:
            self._resolved_user = resolve_hyperliquid_user(
                client=self._client,
                account_address=self._config.account_address,
                vault_address=self._config.vault_address,
                testnet=self._config.testnet,
                http_timeout_secs=self._config.http_timeout_secs,
                http_proxy_url=self._config.http_proxy_url,
            )
        if not self._client_identity_initialized:
            set_account_id = getattr(self._client, "set_account_id", None)
            if callable(set_account_id):
                set_account_id("HYPERLIQUID-master")
            set_account_address = getattr(self._client, "set_account_address", None)
            account_query_address = getattr(self._resolved_user, "account_query_address", None)
            if callable(set_account_address) and account_query_address is not None:
                set_account_address(account_query_address)
            self._client_identity_initialized = True
        kwargs: dict[str, Any] = {}
        account_query_address = getattr(self._resolved_user, "account_query_address", None)
        if account_query_address is not None:
            kwargs["account_address"] = account_query_address
        if self._config.dex is not None:
            kwargs["dex"] = self._config.dex
        return kwargs

    async def _fetch_snapshot(self) -> dict[str, Any]:
        request_kwargs = self._request_kwargs()
        positions = await self._client.request_position_status_reports(**request_kwargs)
        account_id = "HYPERLIQUID-master"
        account_query_address = _optional_text(request_kwargs.get("account_address"))
        ts_ms = int(time.time() * 1000)
        clearinghouse_payload = _post_hyperliquid_info(
            payload={
                "type": "clearinghouseState",
                "user": account_query_address,
                **({"dex": self._config.dex} if self._config.dex is not None else {}),
            },
            testnet=self._config.testnet,
            timeout_secs=self._config.http_timeout_secs,
            http_proxy_url=self._config.http_proxy_url,
        )
        spot_payload = _post_hyperliquid_info(
            payload={
                "type": "spotClearinghouseState",
                "user": account_query_address,
                **({"dex": self._config.dex} if self._config.dex is not None else {}),
            },
            testnet=self._config.testnet,
            timeout_secs=self._config.http_timeout_secs,
            http_proxy_url=self._config.http_proxy_url,
        )

        raw_positions: list[dict[str, Any]] = []
        for report in positions:
            if hasattr(report, "to_dict"):
                row = dict(report.to_dict())
            elif isinstance(report, Mapping):
                row = dict(report)
            else:
                continue
            row["exchange"] = "hyperliquid"
            row.setdefault("kind", "position")
            row.setdefault("account_id", account_id)
            row.setdefault("account", account_id)
            row.setdefault("ts_ms", row.get("ts_last") or row.get("ts_init") or ts_ms)
            raw_positions.append(row)

        payload = {
            "accounts": [
                {
                    "account_id": account_id,
                    "venue": "hyperliquid",
                    "events": [
                        {
                            "account_id": account_id,
                            "venue": "hyperliquid",
                            "balances": _extract_hyperliquid_cash_balances(
                                clearinghouse_payload=clearinghouse_payload,
                                spot_payload=spot_payload,
                            ),
                            "ts_ms": ts_ms,
                        },
                    ],
                },
            ],
            "positions": raw_positions,
            "ts_ms": ts_ms,
        }
        rows = build_balances_rows(
            raw_snapshot=payload,
            strategy_id="shared_account",
        )
        for row in rows:
            if row.get("kind") != "position":
                continue
            naming = canonical_naming_fields(
                instrument_id=row.get("instrument_id"),
                exchange=row.get("exchange"),
                asset=row.get("asset"),
                is_position=True,
            )
            base_asset = _optional_text(naming.get("base_asset"))
            if base_asset is None:
                continue
            base_asset = base_asset.split(":", maxsplit=1)[-1]
            row["asset"] = base_asset
            row["coin"] = base_asset
            row["base"] = base_asset
        return {
            "source_scope": "shared_account",
            "rows": rows,
            "totals": _extract_hyperliquid_account_totals(clearinghouse_payload),
        }


class BinanceFuturesAccountProjectionProvider:
    def __init__(self, config: BinanceFuturesAccountProjectionProviderConfig) -> None:
        self._config = config
        self._clock = LiveClock()
        self._client = get_cached_binance_http_client(
            clock=self._clock,
            account_type=config.account_type,
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http
            or get_private_http_base_url(
                config.account_type,
                private_api_family=config.private_api_family,
                environment=config.environment,
                is_us=False,
            ),
            environment=config.environment,
            proxy_url=config.http_proxy_url,
        )
        self._http_account = BinanceFuturesAccountHttpAPI(
            client=self._client,
            clock=self._clock,
            account_type=config.account_type,
            private_api_family=config.private_api_family,
        )
        self._latest_snapshot: dict[str, Any] | None = None
        self._last_refresh_monotonic = 0.0

    def stop(self) -> None:
        return None

    def snapshot(self) -> dict[str, Any] | None:
        if self._latest_snapshot is None:
            return None
        return copy.deepcopy(self._latest_snapshot)

    def refresh(self) -> dict[str, Any] | None:
        now = time.monotonic()
        if (
            self._latest_snapshot is not None
            and (now - self._last_refresh_monotonic) < self._config.refresh_interval_secs
        ):
            return self.snapshot()

        try:
            self._latest_snapshot = asyncio.run(self._fetch_snapshot())
            self._last_refresh_monotonic = time.monotonic()
        except Exception as exc:
            _ACCOUNT_PROJECTION_LOG.warning(
                "Binance futures shared-account refresh failed: %s",
                exc,
            )
        return self.snapshot()

    async def _fetch_snapshot(self) -> dict[str, Any]:
        recv_window = str(self._config.recv_window_ms)
        account_info = await self._http_account.query_futures_account_info(
            recv_window=recv_window,
        )
        positions = await self._http_account.query_futures_position_risk(
            recv_window=recv_window,
        )

        account_id = "BINANCE_PERP-master"
        ts_ms = int(time.time() * 1000)
        payload = {
            "market_type": "perp",
            "accounts": [
                {
                    "account_id": account_id,
                    "venue": "binance_perp",
                    "events": [
                        {
                            "account_id": account_id,
                            "venue": "binance_perp",
                            "balances": _extract_binance_futures_balances(account_info),
                            "ts_ms": ts_ms,
                        },
                    ],
                },
            ],
            "positions": _extract_binance_futures_positions(
                positions,
                account_id=account_id,
            ),
            "ts_ms": ts_ms,
        }
        rows = build_balances_rows(
            raw_snapshot=payload,
            strategy_id="shared_account",
        )
        return {
            "source_scope": "shared_account",
            "rows": rows,
            "totals": _extract_binance_account_totals(account_info),
        }


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

    if dockerized_gateway is not None and not dockerized_gateway.manage_container:
        dockerized_gateway = None

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


def _build_hyperliquid_account_provider(
    *,
    scope_config: AccountScopeConfig,
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> HyperliquidAccountProjectionProvider:
    _ = (account_scope_id, source_strategy_ids)
    return HyperliquidAccountProjectionProvider(
        HyperliquidAccountProjectionProviderConfig(
            private_key=_env_value(scope_config.private_key_env),
            account_address=_env_value(scope_config.account_address_env),
            vault_address=_env_value(scope_config.vault_address_env),
            dex=scope_config.dex,
            testnet=scope_config.testnet,
            http_timeout_secs=scope_config.http_timeout_secs or 10,
            http_proxy_url=scope_config.http_proxy_url,
        ),
    )


def _build_binance_futures_account_provider(
    *,
    scope_config: AccountScopeConfig,
    account_scope_id: str,
    source_strategy_ids: tuple[str, ...],
) -> BinanceFuturesAccountProjectionProvider | None:
    _ = (account_scope_id, source_strategy_ids)
    api_key = _env_value(scope_config.api_key_env)
    api_secret = _env_value(scope_config.api_secret_env)
    if api_key is None or api_secret is None:
        return None

    return BinanceFuturesAccountProjectionProvider(
        BinanceFuturesAccountProjectionProviderConfig(
            api_key=api_key,
            api_secret=api_secret,
            account_type=_parse_binance_account_type(scope_config.account_type),
            private_api_family=_parse_binance_private_api_family(scope_config.private_api_family),
            environment=BinanceEnvironment.TESTNET if scope_config.testnet else BinanceEnvironment.LIVE,
            base_url_http=scope_config.base_url_http,
            recv_window_ms=scope_config.recv_window_ms or 5000,
            http_proxy_url=scope_config.http_proxy_url,
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
    if provider_id == "hyperliquid":
        return _build_hyperliquid_account_provider(
            scope_config=scope_config,
            account_scope_id=account_scope_id,
            source_strategy_ids=source_strategy_ids,
        )
    if provider_id == "binance":
        return _build_binance_futures_account_provider(
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
