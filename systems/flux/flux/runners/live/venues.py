from __future__ import annotations

import inspect
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.bitget import BITGET
from nautilus_trader.adapters.bitget import BitgetDataClientConfig
from nautilus_trader.adapters.bitget import BitgetExecClientConfig
from nautilus_trader.adapters.bitget import BitgetLiveDataClientFactory
from nautilus_trader.adapters.bitget import BitgetLiveExecClientFactory
from nautilus_trader.adapters.bitget.constants import BITGET_DEFAULT_PRODUCTS
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenExecClientConfig
from nautilus_trader.adapters.kraken import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken import KrakenLiveExecClientFactory
from nautilus_trader.adapters.kraken import KrakenProductType
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.adapters.okx import OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode
from nautilus_trader.core.nautilus_pyo3 import OKXVipLevel
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


FieldCoercer = Callable[[Any, str], Any]


@dataclass(frozen=True)
class VenueAdapterSpec:
    adapter_id: str
    venue_key: Any
    data_config_cls: type[Any]
    data_factory_cls: type[Any]
    exec_config_cls: type[Any] | None
    exec_factory_cls: type[Any] | None
    field_aliases: dict[str, str]
    field_coercers: dict[str, FieldCoercer]
    secret_fields: tuple[tuple[str, str], ...]
    mode_defaults: dict[str, Callable[[str], Any]]
    instrument_provider_factory: Callable[[InstrumentId], Any] | None = None
    multi_venue_execution: bool = False


@dataclass(frozen=True)
class ResolvedStrategyVenues:
    execution_venue: str
    reference_venue: str
    execution_instrument_id: InstrumentId
    reference_instrument_id: InstrumentId
    data_clients: dict[Any, Any]
    data_factories: dict[Any, type[Any]]
    exec_clients: dict[Any, Any]
    exec_factories: dict[Any, type[Any]]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _enum_member(enum_type: Any, raw_value: Any, *, field_name: str) -> Any:
    if isinstance(raw_value, enum_type):
        return raw_value

    name = str(raw_value).strip().upper()
    if not name:
        raise ValueError(f"Missing {field_name}")

    try:
        return enum_type[name]
    except (KeyError, TypeError) as exc:
        try:
            return getattr(enum_type, name)
        except AttributeError as attr_exc:
            raise ValueError(f"Invalid {field_name} {raw_value!r}") from attr_exc
        raise ValueError(f"Invalid {field_name} {raw_value!r}") from exc


def _enum_tuple_member(enum_type: Any, raw_value: Any, *, field_name: str) -> tuple[Any, ...]:
    if raw_value is None:
        return ()
    if isinstance(raw_value, (list, tuple)):
        values = tuple(_enum_member(enum_type, item, field_name=field_name) for item in raw_value)
    else:
        values = (_enum_member(enum_type, raw_value, field_name=field_name),)
    if not values:
        raise ValueError(f"Missing {field_name}")
    return values


def _positive_int(raw_value: Any, *, field_name: str, default: int | None = None) -> int:
    if raw_value is None:
        if default is None:
            raise ValueError(f"Missing {field_name}")
        return default
    value = int(raw_value)
    if value <= 0:
        raise ValueError(f"Invalid {field_name} (must be > 0)")
    return value


def _bool_value(raw_value: Any, *, default: bool) -> bool:
    if raw_value is None:
        return default
    return bool(raw_value)


def _resolve_secret(section: dict[str, Any], *, value_key: str, env_key: str) -> str | None:
    inline = _optional_text(section.get(value_key))
    if inline:
        return inline

    env_name = _optional_text(section.get(env_key))
    if not env_name:
        return None

    import os

    value = os.getenv(env_name)
    if value is None:
        return None
    return value


def _signature_parameter_names(config_cls: type[Any]) -> set[str]:
    return set(inspect.signature(config_cls).parameters)


def _coerce_bybit_product_types(raw_value: Any, venue_name: str) -> tuple[Any, ...]:
    return _enum_tuple_member(
        BybitProductType,
        raw_value,
        field_name=f"node.venues.{venue_name}.product_type",
    )


def _coerce_bitget_product_types(raw_value: Any, venue_name: str) -> tuple[Any, ...]:
    return _enum_tuple_member(
        type(BITGET_DEFAULT_PRODUCTS[0]),
        raw_value,
        field_name=f"node.venues.{venue_name}.product_type",
    )


def _coerce_binance_account_type(raw_value: Any, venue_name: str) -> Any:
    return _enum_member(
        BinanceAccountType,
        raw_value,
        field_name=f"node.venues.{venue_name}.account_type",
    )


def _default_binance_environment(mode: str) -> Any:
    if mode == "live":
        return None
    return BinanceEnvironment.TESTNET


def _coerce_recv_window_ms(raw_value: Any, venue_name: str) -> int:
    return _positive_int(raw_value, field_name=f"node.venues.{venue_name}.recv_window_ms")


def _coerce_kraken_environment(raw_value: Any, venue_name: str) -> Any:
    return _enum_member(
        KrakenEnvironment,
        raw_value,
        field_name=f"node.venues.{venue_name}.environment",
    )


def _coerce_kraken_product_types(raw_value: Any, venue_name: str) -> tuple[Any, ...]:
    return _enum_tuple_member(
        KrakenProductType,
        raw_value,
        field_name=f"node.venues.{venue_name}.product_type",
    )


def _coerce_okx_instrument_types(raw_value: Any, venue_name: str) -> tuple[Any, ...]:
    return _enum_tuple_member(
        OKXInstrumentType,
        raw_value,
        field_name=f"node.venues.{venue_name}.instrument_type",
    )


def _coerce_okx_contract_types(raw_value: Any, venue_name: str) -> tuple[Any, ...]:
    return _enum_tuple_member(
        OKXContractType,
        raw_value,
        field_name=f"node.venues.{venue_name}.contract_type",
    )


def _coerce_okx_margin_mode(raw_value: Any, venue_name: str) -> Any:
    return _enum_member(
        OKXMarginMode,
        raw_value,
        field_name=f"node.venues.{venue_name}.margin_mode",
    )


def _coerce_okx_vip_level(raw_value: Any, venue_name: str) -> Any:
    return _enum_member(
        OKXVipLevel,
        raw_value,
        field_name=f"node.venues.{venue_name}.vip_level",
    )


def _load_interactive_brokers_spec() -> VenueAdapterSpec:
    from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
    from nautilus_trader.adapters.interactive_brokers.config import (
        DockerizedIBGatewayConfig,
    )
    from nautilus_trader.adapters.interactive_brokers.config import (
        InteractiveBrokersDataClientConfig,
    )
    from nautilus_trader.adapters.interactive_brokers.config import (
        InteractiveBrokersExecClientConfig,
    )
    from nautilus_trader.adapters.interactive_brokers.config import (
        InteractiveBrokersInstrumentProviderConfig,
    )
    from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
    from nautilus_trader.adapters.interactive_brokers.factories import (
        InteractiveBrokersLiveDataClientFactory,
    )
    from nautilus_trader.adapters.interactive_brokers.factories import (
        InteractiveBrokersLiveExecClientFactory,
    )

    def _coerce_ibg_client_id(raw_value: Any, venue_name: str) -> int:
        return _positive_int(raw_value, field_name=f"node.venues.{venue_name}.ibg_client_id")

    def _coerce_dockerized_gateway(raw_value: Any, venue_name: str) -> Any:
        if raw_value is None or isinstance(raw_value, DockerizedIBGatewayConfig):
            return raw_value
        if not isinstance(raw_value, dict):
            raise ValueError(
                f"node.venues.{venue_name}.dockerized_gateway must be a TOML table",
            )
        return DockerizedIBGatewayConfig(**raw_value)

    def _build_instrument_provider(instrument_id: InstrumentId) -> Any:
        return InteractiveBrokersInstrumentProviderConfig(
            load_ids=frozenset([instrument_id]),
            symbology_method=SymbologyMethod.IB_SIMPLIFIED,
        )

    return VenueAdapterSpec(
        adapter_id="interactive_brokers",
        venue_key=IB_VENUE,
        data_config_cls=InteractiveBrokersDataClientConfig,
        data_factory_cls=InteractiveBrokersLiveDataClientFactory,
        exec_config_cls=InteractiveBrokersExecClientConfig,
        exec_factory_cls=InteractiveBrokersLiveExecClientFactory,
        field_aliases={},
        field_coercers={
            "ibg_client_id": _coerce_ibg_client_id,
            "dockerized_gateway": _coerce_dockerized_gateway,
        },
        secret_fields=(),
        mode_defaults={},
        instrument_provider_factory=_build_instrument_provider,
        multi_venue_execution=True,
    )


SUPPORTED_VENUE_ADAPTERS: dict[str, VenueAdapterSpec] = {
    "binance": VenueAdapterSpec(
        adapter_id="binance",
        venue_key=BINANCE,
        data_config_cls=BinanceDataClientConfig,
        data_factory_cls=BinanceLiveDataClientFactory,
        exec_config_cls=BinanceExecClientConfig,
        exec_factory_cls=BinanceLiveExecClientFactory,
        field_aliases={},
        field_coercers={
            "account_type": _coerce_binance_account_type,
        },
        secret_fields=(("api_key", "api_key_env"), ("api_secret", "api_secret_env")),
        mode_defaults={"environment": _default_binance_environment},
    ),
    "bitget": VenueAdapterSpec(
        adapter_id="bitget",
        venue_key=BITGET,
        data_config_cls=BitgetDataClientConfig,
        data_factory_cls=BitgetLiveDataClientFactory,
        exec_config_cls=BitgetExecClientConfig,
        exec_factory_cls=BitgetLiveExecClientFactory,
        field_aliases={"product_types": "product_type"},
        field_coercers={
            "product_types": _coerce_bitget_product_types,
        },
        secret_fields=(
            ("api_key", "api_key_env"),
            ("api_secret", "api_secret_env"),
            ("api_passphrase", "api_passphrase_env"),
        ),
        mode_defaults={"demo": lambda mode: mode != "live"},
    ),
    "bybit": VenueAdapterSpec(
        adapter_id="bybit",
        venue_key=BYBIT,
        data_config_cls=BybitDataClientConfig,
        data_factory_cls=BybitLiveDataClientFactory,
        exec_config_cls=BybitExecClientConfig,
        exec_factory_cls=BybitLiveExecClientFactory,
        field_aliases={"product_types": "product_type"},
        field_coercers={
            "product_types": _coerce_bybit_product_types,
            "recv_window_ms": _coerce_recv_window_ms,
        },
        secret_fields=(("api_key", "api_key_env"), ("api_secret", "api_secret_env")),
        mode_defaults={"testnet": lambda mode: mode != "live"},
    ),
    "hyperliquid": VenueAdapterSpec(
        adapter_id="hyperliquid",
        venue_key=HYPERLIQUID,
        data_config_cls=HyperliquidDataClientConfig,
        data_factory_cls=HyperliquidLiveDataClientFactory,
        exec_config_cls=HyperliquidExecClientConfig,
        exec_factory_cls=HyperliquidLiveExecClientFactory,
        field_aliases={},
        field_coercers={},
        secret_fields=(
            ("private_key", "private_key_env"),
            ("account_address", "account_address_env"),
            ("vault_address", "vault_address_env"),
        ),
        mode_defaults={"testnet": lambda mode: mode != "live"},
    ),
    "kraken": VenueAdapterSpec(
        adapter_id="kraken",
        venue_key=KRAKEN,
        data_config_cls=KrakenDataClientConfig,
        data_factory_cls=KrakenLiveDataClientFactory,
        exec_config_cls=KrakenExecClientConfig,
        exec_factory_cls=KrakenLiveExecClientFactory,
        field_aliases={"product_types": "product_type"},
        field_coercers={
            "environment": _coerce_kraken_environment,
            "product_types": _coerce_kraken_product_types,
        },
        secret_fields=(("api_key", "api_key_env"), ("api_secret", "api_secret_env")),
        mode_defaults={},
    ),
    "okx": VenueAdapterSpec(
        adapter_id="okx",
        venue_key=OKX,
        data_config_cls=OKXDataClientConfig,
        data_factory_cls=OKXLiveDataClientFactory,
        exec_config_cls=OKXExecClientConfig,
        exec_factory_cls=OKXLiveExecClientFactory,
        field_aliases={
            "instrument_types": "instrument_type",
            "contract_types": "contract_type",
        },
        field_coercers={
            "instrument_types": _coerce_okx_instrument_types,
            "contract_types": _coerce_okx_contract_types,
            "margin_mode": _coerce_okx_margin_mode,
            "vip_level": _coerce_okx_vip_level,
        },
        secret_fields=(
            ("api_key", "api_key_env"),
            ("api_secret", "api_secret_env"),
            ("api_passphrase", "api_passphrase_env"),
        ),
        mode_defaults={"is_demo": lambda mode: mode != "live"},
    ),
}

try:
    _interactive_brokers_spec = _load_interactive_brokers_spec()
except ImportError:
    _interactive_brokers_spec = None
else:
    SUPPORTED_VENUE_ADAPTERS["interactive_brokers"] = _interactive_brokers_spec
    SUPPORTED_VENUE_ADAPTERS["ibkr"] = _interactive_brokers_spec


def _instrument_id_from_entry(
    entry: dict[str, Any],
    *,
    field_name: str,
    venue_name: str,
) -> InstrumentId:
    raw_value = _optional_text(entry.get("instrument_id"))
    if not raw_value:
        raise ValueError(f"`{field_name}` requires `node.venues.{venue_name}.instrument_id`")
    return InstrumentId.from_str(raw_value)


def _build_client_config(
    *,
    spec: VenueAdapterSpec,
    config_cls: type[Any],
    venue_name: str,
    venue_cfg: dict[str, Any],
    instrument_id: InstrumentId,
    mode: str,
    default_routing: bool,
) -> Any:
    parameter_names = _signature_parameter_names(config_cls)
    kwargs: dict[str, Any] = {}
    routing_venues = frozenset({venue_name, str(instrument_id.venue).upper()})

    if "instrument_provider" in parameter_names:
        if spec.instrument_provider_factory is not None:
            kwargs["instrument_provider"] = spec.instrument_provider_factory(instrument_id)
        else:
            kwargs["instrument_provider"] = InstrumentProviderConfig(
                load_ids=frozenset([instrument_id]),
            )
    if "routing" in parameter_names:
        kwargs["routing"] = RoutingConfig(default=default_routing, venues=routing_venues)
    if "venue" in parameter_names:
        kwargs["venue"] = Venue(venue_name)

    for value_key, env_key in spec.secret_fields:
        if value_key not in parameter_names:
            continue
        secret = _resolve_secret(venue_cfg, value_key=value_key, env_key=env_key)
        if secret is not None:
            kwargs[value_key] = secret

    for field_name in parameter_names:
        if field_name in {"instrument_provider", "routing", "venue"}:
            continue
        if field_name in kwargs:
            continue

        raw_value = venue_cfg.get(field_name)
        if raw_value is None:
            alias = spec.field_aliases.get(field_name)
            if alias is not None:
                raw_value = venue_cfg.get(alias)

        if raw_value is None:
            default_factory = spec.mode_defaults.get(field_name)
            if default_factory is not None:
                kwargs[field_name] = default_factory(mode)
            continue

        coercer = spec.field_coercers.get(field_name)
        kwargs[field_name] = coercer(raw_value, venue_name) if coercer is not None else raw_value

    return config_cls(**kwargs)


def _legacy_node_venues(config: dict[str, Any]) -> dict[str, dict[str, Any]]:
    node_cfg = config.get("node", {})
    if not isinstance(node_cfg, dict):
        raise ValueError("[node] must be a TOML table")

    out: dict[str, dict[str, Any]] = {}
    maker_instrument_id = (
        _optional_text(node_cfg.get("maker_instrument_id")) or "PLUMEUSDT-LINEAR.BYBIT"
    )
    reference_instrument_id = (
        _optional_text(node_cfg.get("reference_instrument_id")) or "PLUMEUSDT.BINANCE"
    )

    bybit_cfg = node_cfg.get("bybit")
    if isinstance(bybit_cfg, dict):
        entry = dict(bybit_cfg)
        entry.setdefault("adapter", "bybit")
        entry.setdefault("instrument_id", maker_instrument_id)
        if "product_type" not in entry and "product_types" not in entry:
            entry["product_type"] = "LINEAR"
        out["BYBIT"] = entry

    binance_cfg = node_cfg.get("binance")
    if isinstance(binance_cfg, dict):
        entry = dict(binance_cfg)
        entry.setdefault("adapter", "binance")
        entry.setdefault("instrument_id", reference_instrument_id)
        out["BINANCE"] = entry

    return out


def _node_venues(config: dict[str, Any]) -> dict[str, dict[str, Any]]:
    node_cfg = config.get("node", {})
    if not isinstance(node_cfg, dict):
        raise ValueError("[node] must be a TOML table")

    raw_value = node_cfg.get("venues")
    if raw_value is None:
        return _legacy_node_venues(config)
    if not isinstance(raw_value, dict):
        raise ValueError("[node.venues] must be a TOML table of venue configs")

    out: dict[str, dict[str, Any]] = {}
    for venue_name, venue_cfg in raw_value.items():
        if not isinstance(venue_cfg, dict):
            raise ValueError(f"[node.venues.{venue_name}] must be a TOML table")
        out[str(venue_name).strip().upper()] = venue_cfg
    return out


def _resolve_strategy_venue_names(config: dict[str, Any]) -> tuple[str, str]:
    venues_cfg = config.get("venues", {})
    if not isinstance(venues_cfg, dict):
        raise ValueError("[venues] must be a TOML table")

    execution_venue = _optional_text(venues_cfg.get("execution_venue")) or "BYBIT"
    reference_venue = _optional_text(venues_cfg.get("reference_venue")) or "BINANCE"
    return execution_venue.upper(), reference_venue.upper()


def resolve_strategy_venues(
    *,
    config: dict[str, Any],
    mode: str,
    enable_execution: bool,
) -> ResolvedStrategyVenues:
    execution_venue_name, reference_venue_name = _resolve_strategy_venue_names(config)
    venue_entries = _node_venues(config)

    if execution_venue_name not in venue_entries:
        raise ValueError(
            f"`venues.execution_venue={execution_venue_name}` requires `node.venues.{execution_venue_name}`",
        )
    if reference_venue_name not in venue_entries:
        raise ValueError(
            f"`venues.reference_venue={reference_venue_name}` requires `node.venues.{reference_venue_name}`",
        )

    data_clients: dict[Any, Any] = {}
    data_factories: dict[Any, type[Any]] = {}
    exec_clients: dict[Any, Any] = {}
    exec_factories: dict[Any, type[Any]] = {}
    instrument_ids: dict[str, InstrumentId] = {}
    data_enabled_venues: set[str] = set()
    exec_enabled_venues: set[str] = set()
    venue_records: list[tuple[str, dict[str, Any], VenueAdapterSpec, InstrumentId, bool]] = []

    for venue_name, venue_cfg in venue_entries.items():
        adapter_id = (_optional_text(venue_cfg.get("adapter")) or venue_name.lower()).lower()
        spec = SUPPORTED_VENUE_ADAPTERS.get(adapter_id)
        if spec is None:
            raise ValueError(
                f"Unsupported adapter {adapter_id!r} for node.venues.{venue_name}. "
                f"Supported adapters: {sorted(SUPPORTED_VENUE_ADAPTERS)}",
            )

        instrument_id = _instrument_id_from_entry(
            venue_cfg,
            field_name=f"node.venues.{venue_name}",
            venue_name=venue_name,
        )
        instrument_ids[venue_name] = instrument_id

        venue_execution_enabled = _bool_value(
            venue_cfg.get("execution"),
            default=venue_name == execution_venue_name,
        )
        venue_records.append((venue_name, venue_cfg, spec, instrument_id, venue_execution_enabled))

    suppress_primary_exec_default_routing = any(
        enable_execution
        and venue_execution_enabled
        and venue_name != execution_venue_name
        and spec.multi_venue_execution
        for venue_name, _venue_cfg, spec, _instrument_id, venue_execution_enabled in venue_records
    )

    for venue_name, venue_cfg, spec, instrument_id, venue_execution_enabled in venue_records:
        if _bool_value(venue_cfg.get("data"), default=True):
            data_clients[venue_name] = _build_client_config(
                spec=spec,
                config_cls=spec.data_config_cls,
                venue_name=venue_name,
                venue_cfg=venue_cfg,
                instrument_id=instrument_id,
                mode=mode,
                default_routing=venue_name == execution_venue_name,
            )
            data_factories[venue_name] = spec.data_factory_cls
            data_enabled_venues.add(venue_name)

        if enable_execution and venue_execution_enabled:
            if spec.exec_config_cls is None or spec.exec_factory_cls is None:
                raise ValueError(
                    f"Adapter {adapter_id!r} does not support execution for node.venues.{venue_name}",
                )
            exec_clients[venue_name] = _build_client_config(
                spec=spec,
                config_cls=spec.exec_config_cls,
                venue_name=venue_name,
                venue_cfg=venue_cfg,
                instrument_id=instrument_id,
                mode=mode,
                default_routing=(
                    venue_name == execution_venue_name and not suppress_primary_exec_default_routing
                ),
            )
            exec_factories[venue_name] = spec.exec_factory_cls
            exec_enabled_venues.add(venue_name)

    if execution_venue_name not in data_enabled_venues:
        raise ValueError(
            f"`venues.execution_venue={execution_venue_name}` must have a data-enabled client in `[node.venues.{execution_venue_name}]`",
        )
    if reference_venue_name not in data_enabled_venues:
        raise ValueError(
            f"`venues.reference_venue={reference_venue_name}` must have a data-enabled client in `[node.venues.{reference_venue_name}]`",
        )
    if enable_execution and execution_venue_name not in exec_enabled_venues:
        raise ValueError(
            f"`venues.execution_venue={execution_venue_name}` must have execution enabled in `[node.venues.{execution_venue_name}]`",
        )

    return ResolvedStrategyVenues(
        execution_venue=execution_venue_name,
        reference_venue=reference_venue_name,
        execution_instrument_id=instrument_ids[execution_venue_name],
        reference_instrument_id=instrument_ids[reference_venue_name],
        data_clients=data_clients,
        data_factories=data_factories,
        exec_clients=exec_clients,
        exec_factories=exec_factories,
    )
