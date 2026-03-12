from __future__ import annotations

from typing import Any

import pytest

from nautilus_trader.flux.common.account_projection import ProfileAccountProviderBinding
from nautilus_trader.flux.common.keys import FluxRedisKeys


class _FakeAccountProjectionProvider:
    def __init__(
        self,
        *,
        rows: list[dict[str, Any]],
    ) -> None:
        self._rows = rows

    def snapshot(self) -> dict[str, Any] | None:
        return {
            "rows": list(self._rows),
        }


def test_profile_account_projection_publishes_ibkr_positions_without_strategy_snapshots() -> None:
    from nautilus_trader.flux.common.account_projection import build_profile_account_snapshot

    provider = _FakeAccountProjectionProvider(
        rows=[
            {
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "AAPL",
                "kind": "position",
                "signed_qty": "25",
            },
        ],
    )

    snapshot = build_profile_account_snapshot(
        profile_id="equities",
        bindings=[
            ProfileAccountProviderBinding(
                account_scope_id="ibkr.reference.main",
                source_strategy_ids=("aapl_tradexyz_makerv3",),
                provider=provider,
            ),
        ],
        ts_ms=1_700_000_000_000,
    )

    assert snapshot["account_scope_ids"] == ["ibkr.reference.main"]
    assert snapshot["rows"][0]["exchange"] == "ibkr"
    assert snapshot["rows"][0]["source_scope"] == "shared_account"
    assert snapshot["rows"][0]["account_scope_id"] == "ibkr.reference.main"
    assert snapshot["rows"][0]["source_strategy_ids"] == ["aapl_tradexyz_makerv3"]


def test_profile_account_projection_assigns_scope_stable_row_ids() -> None:
    from nautilus_trader.flux.common.account_projection import build_profile_account_snapshot

    snapshot = build_profile_account_snapshot(
        profile_id="equities",
        bindings=[
            ProfileAccountProviderBinding(
                account_scope_id="hyperliquid.xyz.main",
                source_strategy_ids=("aapl_tradexyz_makerv3",),
                provider=_FakeAccountProjectionProvider(
                    rows=[
                        {
                            "row_id": "shared_account:acc:0:evt:0:0",
                            "exchange": "hyperliquid",
                            "account": "HYPERLIQUID-master",
                            "asset": "USDC",
                            "total": "0",
                        },
                    ],
                ),
            ),
            ProfileAccountProviderBinding(
                account_scope_id="ibkr.reference.main",
                source_strategy_ids=("aapl_tradexyz_makerv3",),
                provider=_FakeAccountProjectionProvider(
                    rows=[
                        {
                            "row_id": "shared_account:acc:0:evt:0:0",
                            "exchange": "ibkr",
                            "account": "U1234567",
                            "asset": "HKD",
                            "total": "85671.33",
                        },
                    ],
                ),
            ),
        ],
        ts_ms=1_700_000_000_000,
    )

    row_ids = {row["row_id"] for row in snapshot["rows"]}

    assert row_ids == {
        "equities:shared:hyperliquid.xyz.main:cash:hyperliquid:HYPERLIQUID-master:USDC",
        "equities:shared:ibkr.reference.main:cash:ibkr:U1234567:HKD",
    }


def test_profile_account_projection_round_trip_preserves_rows_and_scope_keys() -> None:
    from nautilus_trader.flux.common.account_projection import build_profile_account_snapshot
    from nautilus_trader.flux.common.account_projection import decode_profile_account_snapshot
    from nautilus_trader.flux.common.account_projection import encode_profile_account_snapshot

    snapshot = build_profile_account_snapshot(
        profile_id="equities",
        bindings=[
            ProfileAccountProviderBinding(
                account_scope_id="ibkr.reference.main",
                source_strategy_ids=("aapl_tradexyz_makerv3",),
                provider=_FakeAccountProjectionProvider(
                    rows=[
                        {
                            "exchange": "ibkr",
                            "account": "U1234567",
                            "asset": "USD",
                            "total": "12345.67",
                        },
                    ],
                ),
            ),
        ],
        ts_ms=1_700_000_000_123,
    )

    encoded = encode_profile_account_snapshot(snapshot)
    decoded = decode_profile_account_snapshot(encoded)

    assert decoded == snapshot
    assert (
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
        )
        == "flux:v1:profile:account_projection:equities:ibkr.reference.main"
    )


def test_account_scope_decoder_requires_provider_and_scope_id() -> None:
    from nautilus_trader.flux.common.account_scopes import decode_account_scopes

    with pytest.raises(ValueError, match="provider"):
        decode_account_scopes(
            [
                {
                    "scope_id": "ibkr.reference.main",
                    "venue": "IBKR",
                },
            ],
        )

    with pytest.raises(ValueError, match="scope_id"):
        decode_account_scopes(
            [
                {
                    "scope_id": " ",
                    "provider": "ibkr",
                    "venue": "IBKR",
                },
            ],
        )
