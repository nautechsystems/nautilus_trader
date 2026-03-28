from __future__ import annotations

import importlib
from decimal import Decimal
from pathlib import Path

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_attribution_module():
    path = _repo_root() / "systems/flux/flux/execution/attribution.py"
    assert path.exists(), "attribution module should exist"
    return importlib.import_module("flux.execution.attribution")


def test_allocate_shared_netting_fill_consumes_reservations_in_explicit_sequence_order() -> None:
    attribution = _load_attribution_module()

    result = attribution.allocate_shared_netting_fill(
        controller_scope_id="ibkr.hedge.main",
        fill_qty="70",
        reservations=(
            attribution.AttributionReservation(
                strategy_id="strategy-b",
                reserved_qty="60",
                reservation_seq=1,
            ),
            attribution.AttributionReservation(
                strategy_id="strategy-a",
                reserved_qty="40",
                reservation_seq=2,
            ),
        ),
    )

    assert result.controller_scope_id == "ibkr.hedge.main"
    assert result.fill_qty == Decimal("70")
    assert result.unattributed_qty == Decimal("0")
    assert [
        (
            allocation.strategy_id,
            allocation.attributed_qty,
            allocation.remaining_reservation_qty,
            allocation.reservation_seq,
        )
        for allocation in result.allocations
    ] == [
        ("strategy-b", Decimal("60"), Decimal("0"), 1),
        ("strategy-a", Decimal("10"), Decimal("30"), 2),
    ]


def test_allocate_shared_netting_fill_tracks_unattributed_residual_when_reservations_are_short() -> None:
    attribution = _load_attribution_module()

    result = attribution.allocate_shared_netting_fill(
        controller_scope_id="ibkr.hedge.main",
        fill_qty="-25",
        reservations=(
            attribution.AttributionReservation(
                strategy_id="strategy-short",
                reserved_qty="-10",
                reservation_seq=1,
            ),
        ),
    )

    assert result.fill_qty == Decimal("-25")
    assert result.unattributed_qty == Decimal("-15")
    assert len(result.allocations) == 1
    assert result.allocations[0].strategy_id == "strategy-short"
    assert result.allocations[0].attributed_qty == Decimal("-10")
    assert result.allocations[0].remaining_reservation_qty == Decimal("0")


def test_allocate_shared_netting_fill_rejects_duplicate_strategy_reservations() -> None:
    attribution = _load_attribution_module()

    with pytest.raises(ValueError, match="duplicate strategy_id"):
        attribution.allocate_shared_netting_fill(
            controller_scope_id="ibkr.hedge.main",
            fill_qty="5",
            reservations=(
                attribution.AttributionReservation(
                    strategy_id="strategy-a",
                    reserved_qty="3",
                    reservation_seq=1,
                ),
                attribution.AttributionReservation(
                    strategy_id="strategy-a",
                    reserved_qty="2",
                    reservation_seq=2,
                ),
            ),
        )


def test_allocate_shared_netting_fill_rejects_mixed_sign_reservations() -> None:
    attribution = _load_attribution_module()

    with pytest.raises(ValueError, match="same sign as fill_qty"):
        attribution.allocate_shared_netting_fill(
            controller_scope_id="ibkr.hedge.main",
            fill_qty="5",
            reservations=(
                attribution.AttributionReservation(
                    strategy_id="strategy-a",
                    reserved_qty="-5",
                    reservation_seq=1,
                ),
            ),
        )
