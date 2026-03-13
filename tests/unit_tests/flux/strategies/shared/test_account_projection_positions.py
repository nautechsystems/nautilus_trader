from __future__ import annotations

from typing import Any

from nautilus_trader.flux.common.account_projection import encode_profile_account_snapshot
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.strategies.shared.account_projection_positions import (
    read_matching_shared_account_position_row,
)


class _FakeRedis:
    def __init__(self, values: dict[str, str | bytes] | None = None) -> None:
        self._values: dict[str, bytes] = {}
        for key, value in (values or {}).items():
            self._values[key] = value.encode() if isinstance(value, str) else value

    def get(self, key: str) -> bytes | None:
        return self._values.get(key)


def _projection_key(*, profile_id: str, account_scope_id: str) -> str:
    return FluxRedisKeys.profile_account_projection(
        profile_id=profile_id,
        account_scope_id=account_scope_id,
        namespace="flux",
        schema_version="v1",
    )


def _encode_snapshot(*, rows: list[dict[str, Any]]) -> str:
    return encode_profile_account_snapshot(
        {
            "profile_id": "equities",
            "account_scope_ids": ["hyperliquid.xyz.main"],
            "rows": rows,
            "totals": {},
            "server_ts_ms": 1_700_000_000_999,
        },
    )


def test_read_matching_shared_account_position_row_returns_freshest_exact_instrument_match() -> None:
    redis_client = _FakeRedis(
        {
            _projection_key(
                profile_id="equities",
                account_scope_id="hyperliquid.xyz.main",
            ): _encode_snapshot(
                rows=[
                    {
                        "kind": "position",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "instrument_id": "XYZ:NVDA-USD-PERP.HYPERLIQUID",
                        "signed_qty_venue": "-9.111",
                        "ts_ms": 1_700_000_000_111,
                    },
                    {
                        "kind": "position",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "instrument_id": "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
                        "signed_qty_venue": "-5",
                        "ts_ms": 1_700_000_000_122,
                    },
                    {
                        "kind": "position",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "instrument_id": "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
                        "signed_qty_venue": "-6",
                        "ts_ms": 1_700_000_000_123,
                    },
                ],
            ),
        },
    )

    row = read_matching_shared_account_position_row(
        redis_client=redis_client,
        profile_id="equities",
        account_scope_id="hyperliquid.xyz.main",
        instrument_id="XYZ:GOOGL-USD-PERP.HYPERLIQUID",
        namespace="flux",
        schema_version="v1",
    )

    assert row is not None
    assert row["instrument_id"] == "XYZ:GOOGL-USD-PERP.HYPERLIQUID"
    assert row["signed_qty_venue"] == "-6"
    assert row["ts_ms"] == 1_700_000_000_123


def test_read_matching_shared_account_position_row_ignores_non_position_and_non_matching_rows() -> None:
    redis_client = _FakeRedis(
        {
            _projection_key(
                profile_id="equities",
                account_scope_id="hyperliquid.xyz.main",
            ): _encode_snapshot(
                rows=[
                    {
                        "kind": "cash",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "asset": "USDE",
                        "total": "1000",
                        "ts_ms": 1_700_000_000_100,
                    },
                    {
                        "kind": "position",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "instrument_id": "XYZ:COIN-USD-PERP.HYPERLIQUID",
                        "signed_qty_venue": "-22.715",
                        "ts_ms": 1_700_000_000_101,
                    },
                ],
            ),
        },
    )

    row = read_matching_shared_account_position_row(
        redis_client=redis_client,
        profile_id="equities",
        account_scope_id="hyperliquid.xyz.main",
        instrument_id="XYZ:GOOGL-USD-PERP.HYPERLIQUID",
        namespace="flux",
        schema_version="v1",
    )

    assert row is None
