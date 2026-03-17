# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from collections.abc import Callable

import msgspec.structs

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt


_SUPPORTED_ACCOUNT_MODES = ("UTA",)
_SUPPORTED_MARGIN_MODES = ("cross",)
_SUPPORTED_POSITION_MODES = ("one_way",)


def _normalize_optional_value(value: object, transform: Callable[[str], str]) -> str | None:
    if value is None:
        return None

    normalized = transform(str(value).strip())
    return normalized or None


def _validate_supported_value(
    field_name: str,
    value: object,
    *,
    transform: Callable[[str], str],
    supported: tuple[str, ...],
) -> str | None:
    normalized = _normalize_optional_value(value, transform)
    if normalized is None:
        return None
    if normalized not in supported:
        supported_values = ", ".join(supported)
        raise ValueError(
            f"unsupported {field_name}: expected one of ({supported_values}), was {value!r}",
        )
    return normalized


class BitgetDataClientConfig(LiveDataClientConfig):
    """Configuration for ``BitgetDataClient`` instances."""

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    product_types: tuple[object, ...] | None = None
    base_url_http: str | None = None
    base_url_ws_public: str | None = None
    base_url_ws_private: str | None = None
    demo: bool = False
    update_instruments_interval_mins: PositiveInt | None = 60
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None


class BitgetExecClientConfig(LiveExecClientConfig):
    """Configuration for ``BitgetExecutionClient`` instances."""

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    product_types: tuple[object, ...] | None = None
    base_url_http: str | None = None
    base_url_ws_private: str | None = None
    account_mode: str | None = None
    allow_cash_borrowing: bool = False
    margin_mode: str | None = None
    position_mode: str | None = None
    demo: bool = False
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None

    def __post_init__(self) -> None:
        account_mode = _validate_supported_value(
            "account_mode",
            self.account_mode,
            transform=str.upper,
            supported=_SUPPORTED_ACCOUNT_MODES,
        )
        msgspec.structs.force_setattr(
            self,
            "account_mode",
            account_mode,
        )
        msgspec.structs.force_setattr(
            self,
            "margin_mode",
            _validate_supported_value(
                "margin_mode",
                self.margin_mode,
                transform=str.lower,
                supported=_SUPPORTED_MARGIN_MODES,
            ),
        )
        msgspec.structs.force_setattr(
            self,
            "position_mode",
            _validate_supported_value(
                "position_mode",
                self.position_mode,
                transform=lambda value: value.lower().replace("-", "_").replace(" ", "_"),
                supported=_SUPPORTED_POSITION_MODES,
            ),
        )
        if self.allow_cash_borrowing and account_mode != "UTA":
            raise ValueError("allow_cash_borrowing requires account_mode='UTA'")
