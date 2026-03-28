from __future__ import annotations

import json
from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.execution.controller import VenueActivityOrigin
from nautilus_trader.flux.execution.events import ExecutionLifecycleEvent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState
from nautilus_trader.flux.execution.intents import build_client_order_id
from nautilus_trader.flux.strategies.makerv4.managed_orders import HedgeBacklogState
from nautilus_trader.flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
from nautilus_trader.flux.strategies.makerv4.strategy import ControllerIntentCommand
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


def _config(**overrides) -> MakerV4StrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_makerv4",
        "strategy_id": "aapl_tradexyz_makerv4",
        "controller_scope_id": "equities.ibkr.hedge.main",
    }
    base.update(overrides)
    return MakerV4StrategyConfig(**base)


def _instrument(*, raw_symbol: str) -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        price_precision=2,
        price_increment=Decimal("0.01"),
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="USD"),
        multiplier=Decimal("1"),
        is_inverse=False,
        info={},
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


class _FakeRedis:
    def __init__(self) -> None:
        self.payloads: dict[str, bytes] = {}

    def get(self, key: str):
        return self.payloads.get(key)

    def set(self, key: str, value: bytes) -> None:
        self.payloads[key] = value


def _lifecycle_event(
    command: ControllerIntentCommand,
    *,
    controller_seq: int,
    lifecycle_state: ExecutionLifecycleState = ExecutionLifecycleState.WORKING,
) -> ExecutionLifecycleEvent:
    client_order_id = build_client_order_id(
        controller_scope_id=command.intent.controller_scope_id,
        controller_epoch=9,
        controller_seq=controller_seq,
        intent_id=command.intent.intent_id,
    )
    return ExecutionLifecycleEvent(
        intent_id=command.intent.intent_id,
        controller_scope_id=command.intent.controller_scope_id,
        strategy_id=command.intent.strategy_id,
        controller_epoch=9,
        controller_seq=controller_seq,
        client_order_id=client_order_id,
        venue_order_id=f"VENUE-{controller_seq}",
        lifecycle_state=lifecycle_state,
        venue_activity_origin=VenueActivityOrigin.CONTROLLER,
    )


def _canonical_state_payload(*, client_order_id: str, quantity: str) -> dict[str, object]:
    return {
        "controller_scope_id": "equities.ibkr.hedge.main",
        "controller_epoch": 9,
        "controller_seq": 41,
        "authority_state": "controller",
        "stale": False,
        "managed_maker_orders": [
            {
                "client_order_id": client_order_id,
                "instrument_id": str(InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")),
                "side": "BUY",
                "quantity": quantity,
                "price": "192.00",
                "post_only": True,
                "pending_cancel": False,
            },
        ],
    }


def test_makerv4_place_actions_publish_controller_intents_before_shadow_state_exists() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    strategy.submit_order = lambda *_args, **_kwargs: pytest.fail(
        "submit_order should not be used on controller-managed canary lanes",
    )

    strategy._submit_maker_quote(side="BUY", target_price=Decimal("190.01"))

    assert len(published) == 1
    command = published[0]
    assert command.command_type == "place"
    assert command.order_role == "maker"
    assert command.instrument_id == str(strategy.config.maker_instrument_id)
    assert command.side == "BUY"
    assert command.quantity == "1"
    assert command.limit_price == "190.01"
    assert strategy._managed_maker_orders == {}

    strategy.apply_controller_lifecycle_event(_lifecycle_event(command, controller_seq=11))

    assert strategy._managed_maker_orders["BUY"].client_order_id == build_client_order_id(
        controller_scope_id="equities.ibkr.hedge.main",
        controller_epoch=9,
        controller_seq=11,
        intent_id=command.intent.intent_id,
    )
    assert strategy._managed_maker_orders["BUY"].price == Decimal("190.01")


def test_makerv4_cancel_actions_publish_controller_intents_and_wait_for_callbacks() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy.cancel_all_orders = lambda *_args, **_kwargs: pytest.fail(
        "cancel_all_orders should not be used on controller-managed canary lanes",
    )
    strategy._managed_maker_orders = {
        "BUY": ManagedMakerOrderState(
            client_order_id="managed-buy-1",
            instrument_id=str(strategy.config.maker_instrument_id),
            side="BUY",
            quantity=Decimal("1"),
            price=Decimal("190.01"),
            post_only=True,
        ),
    }

    strategy._cancel_managed_maker_orders()

    assert len(published) == 1
    command = published[0]
    assert command.command_type == "cancel"
    assert command.target_client_order_id == "managed-buy-1"
    assert strategy._managed_maker_orders["BUY"].pending_cancel is False

    strategy.apply_controller_lifecycle_event(_lifecycle_event(command, controller_seq=17))

    assert strategy._managed_maker_orders["BUY"].pending_cancel is True


def test_makerv4_hedge_actions_publish_controller_intents_and_store_canonical_exposure() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy.submit_order = lambda *_args, **_kwargs: pytest.fail(
        "submit_order should not be used on controller-managed canary lanes",
    )

    result = strategy._submit_hedge_intent(
        SimpleNamespace(
            instrument_id=str(strategy.config.reference_instrument_id),
            side="BUY",
            qty=Decimal("2"),
            limit_price=Decimal("190.04"),
            time_in_force="IOC",
            outside_rth=True,
            include_overnight=False,
            route="SMART",
            cancel_after_ms=None,
        ),
    )

    assert result is None
    assert len(published) == 1
    command = published[0]
    assert command.command_type == "place"
    assert command.order_role == "hedge"
    assert command.instrument_id == str(strategy.config.reference_instrument_id)
    assert strategy._pending_hedge is None

    strategy.apply_controller_lifecycle_event(_lifecycle_event(command, controller_seq=23))
    strategy.apply_controller_canonical_state(
        {
            "managed_maker_orders": [
                {
                    "client_order_id": build_client_order_id(
                        controller_scope_id="equities.ibkr.hedge.main",
                        controller_epoch=9,
                        controller_seq=23,
                        intent_id=command.intent.intent_id,
                    ),
                    "instrument_id": str(strategy.config.maker_instrument_id),
                    "side": "BUY",
                    "quantity": "3",
                    "price": "191.00",
                    "post_only": True,
                    "pending_cancel": False,
                },
            ],
            "pending_hedge": {
                "fill_id": "fill-77",
                "side": "BUY",
                "requested_qty": "2",
                "remaining_qty": "1",
                "limit_price": "190.04",
                "route": "SMART",
                "time_in_force": "IOC",
                "outside_rth": True,
                "include_overnight": False,
                "cancel_after_ms": None,
                "order_id": build_client_order_id(
                    controller_scope_id="equities.ibkr.hedge.main",
                    controller_epoch=9,
                    controller_seq=23,
                    intent_id=command.intent.intent_id,
                ),
            },
        },
    )

    assert strategy._pending_hedge is not None
    assert strategy._pending_hedge.remaining_qty == Decimal("1")
    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("3")


def test_makerv4_quarantined_lifecycle_clears_pending_intent_bookkeeping() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    strategy._submit_maker_quote(side="BUY", target_price=Decimal("190.01"))
    place_command = published.pop()
    strategy.apply_controller_lifecycle_event(
        _lifecycle_event(
            place_command,
            controller_seq=31,
            lifecycle_state=ExecutionLifecycleState.QUARANTINED,
        ),
    )

    assert strategy._controller_pending_place_intents == {}
    assert strategy._managed_maker_orders == {}

    strategy._managed_maker_orders = {
        "BUY": ManagedMakerOrderState(
            client_order_id="managed-buy-1",
            instrument_id=str(strategy.config.maker_instrument_id),
            side="BUY",
            quantity=Decimal("1"),
            price=Decimal("190.01"),
            post_only=True,
        ),
    }
    strategy._cancel_managed_maker_orders()
    cancel_command = published.pop()
    strategy.apply_controller_lifecycle_event(
        _lifecycle_event(
            cancel_command,
            controller_seq=32,
            lifecycle_state=ExecutionLifecycleState.QUARANTINED,
        ),
    )

    assert strategy._controller_pending_cancel_intents == {}
    assert strategy._controller_pending_cancel_sides == set()
    assert strategy._managed_maker_orders["BUY"].pending_cancel is False


def test_makerv4_hedge_backlog_retry_only_publishes_once_before_callback() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy._latest_quotes = {
        strategy.config.reference_instrument_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    strategy._hedge_backlog = HedgeBacklogState(
        fill_id="backlog-fill-1",
        side="BUY",
        requested_qty=Decimal("2"),
        blocked_reason="stale_quote",
        fill_ts_ms=1_000,
        maker_fee_bps=Decimal("0.25"),
    )

    strategy._retry_hedge_backlog(now_ns=1_050_000_000)
    strategy._retry_hedge_backlog(now_ns=1_060_000_000)

    assert len(published) == 1


def test_makerv4_controller_feed_bridge_sync_once_applies_redis_updates() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    redis_client = _FakeRedis()
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy.configure_controller_canonical_state_feed(
        redis_client=redis_client,
        controller_scope_id="equities.ibkr.hedge.main",
        namespace="flux",
        schema_version="v1",
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy._retry_hedge_backlog = lambda **_kwargs: None
    strategy._refresh_maker_quotes = lambda **_kwargs: None
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy._submit_maker_quote(side="BUY", target_price=Decimal("190.01"))
    command = published[0]
    feed = strategy._controller_canonical_state_feed
    client_order_id = build_client_order_id(
        controller_scope_id="equities.ibkr.hedge.main",
        controller_epoch=9,
        controller_seq=41,
        intent_id=command.intent.intent_id,
    )
    redis_client.payloads[feed.lifecycle_event_key()] = json.dumps(
        _lifecycle_event(command, controller_seq=41).to_dict(),
    ).encode("utf-8")
    redis_client.payloads[feed.canonical_state_key()] = json.dumps(
        _canonical_state_payload(client_order_id=client_order_id, quantity="4"),
    ).encode("utf-8")

    feed.sync_once()

    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("4")
    assert strategy._controller_canonical_state["authority_state"] == "controller"


def test_makerv4_on_start_hydrates_controller_state_without_background_feed_start() -> None:
    strategy = MakerV4Strategy(config=_config())
    redis_client = _FakeRedis()
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=lambda _command: None,
    )
    strategy.configure_controller_canonical_state_feed(
        redis_client=redis_client,
        controller_scope_id="equities.ibkr.hedge.main",
        namespace="flux",
        schema_version="v1",
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._load_runtime_params = lambda: None
    strategy.subscribe_quote_ticks = lambda **_kwargs: None
    strategy._prime_cached_quote = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._reclaim_managed_maker_orders_from_cache = lambda: pytest.fail(
        "controller-managed startup must not reclaim local order state from cache",
    )

    feed = strategy._controller_canonical_state_feed
    redis_client.payloads[feed.canonical_state_key()] = json.dumps(
        _canonical_state_payload(client_order_id="managed-buy-start", quantity="5"),
    ).encode("utf-8")
    feed.start = lambda: pytest.fail(
        "controller-managed startup should not start a background feed worker",
    )

    strategy.on_start()

    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("5")


def test_makerv4_on_quote_tick_refreshes_controller_state_when_due() -> None:
    strategy = MakerV4Strategy(config=_config())
    redis_client = _FakeRedis()
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=lambda _command: None,
    )
    strategy.configure_controller_canonical_state_feed(
        redis_client=redis_client,
        controller_scope_id="equities.ibkr.hedge.main",
        namespace="flux",
        schema_version="v1",
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy._retry_hedge_backlog = lambda **_kwargs: None
    strategy._refresh_maker_quotes = lambda **_kwargs: None
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    feed = strategy._controller_canonical_state_feed
    redis_client.payloads[feed.canonical_state_key()] = json.dumps(
        _canonical_state_payload(client_order_id="managed-buy-tick", quantity="7"),
    ).encode("utf-8")

    strategy.on_quote_tick(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            bid_price=Decimal("190.00"),
            ask_price=Decimal("190.04"),
            ts_event=1_000_000_000,
        ),
    )

    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("7")
