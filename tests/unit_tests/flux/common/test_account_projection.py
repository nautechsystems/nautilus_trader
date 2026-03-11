from __future__ import annotations

from typing import Any

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
