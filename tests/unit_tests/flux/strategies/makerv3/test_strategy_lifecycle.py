from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.portfolio_inventory import decode_component
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory
from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _inventory_runtime_params(**overrides: Decimal | float | str) -> dict[str, Decimal]:
    params: dict[str, Decimal] = {
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal(1),
        "max_skew_bps_global": Decimal(0),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal(1),
        "max_skew_bps_local": Decimal(0),
        "linear_offset_bps": Decimal(0),
    }
    for name, value in overrides.items():
        params[name] = Decimal(str(value))
    return params


class _FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, bytes] = {}

    def get(self, key: str) -> bytes | None:
        return self.values.get(key)

    def set(self, key: str, value: str | bytes) -> bool:
        self.values[key] = value.encode() if isinstance(value, str) else value
        return True


def _okx_linear_perpetual(instrument_id: InstrumentId) -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=instrument_id,
        raw_symbol=Symbol("PLUME-USDT-SWAP"),
        base_currency=Currency.from_str("PLUME"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        is_inverse=False,
        price_precision=6,
        size_precision=0,
        price_increment=Price.from_str("0.000001"),
        size_increment=Quantity.from_str("1"),
        multiplier=Quantity.from_str("10"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "PLUME",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )


def _identity_exposure_instrument(
    instrument_id: InstrumentId,
    *,
    base_currency: str = "PLUME",
) -> SimpleNamespace:
    return SimpleNamespace(
        id=instrument_id,
        base_currency=SimpleNamespace(code=base_currency),
        multiplier=Quantity.from_str("1"),
        info={"base_exposure_mode": "identity"},
        make_qty=lambda value: Quantity.from_str(str(value)),
        make_price=lambda value: Price.from_str(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: qty,
    )


def test_cancel_managed_quotes_idempotency_with_tracked_ids_and_cache_visibility(
    strategy_factory,
) -> None:
    strategy = strategy_factory()

    cached_order = SimpleNamespace(client_order_id="RESTING-1")
    snapshots: list[list[SimpleNamespace]] = [[cached_order], [], []]
    strategy._managed_orders = lambda: snapshots.pop(0)
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")
    assert strategy._managed_client_order_ids == {"RESTING-1"}

    strategy._cancel_managed_quotes("stale")
    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert canceled_all == []
    assert [name for name, _ in events] == ["quotes_canceled", "quotes_canceled", "quotes_canceled"]
    assert events[0][1]["cancel_attempts"] == 1
    assert events[0][1]["cancel_exceptions"] == 0
    assert events[0][1]["cancel_success"] == 1
    assert events[0][1]["cancel_all_instrument"] is False
    assert events[1][1]["cancel_attempts"] == 0
    assert events[1][1]["cancel_exceptions"] == 0
    assert events[1][1]["cancel_success"] == 0
    assert events[1][1]["cancel_all_instrument"] is False
    assert events[2][1]["cancel_attempts"] == 0
    assert events[2][1]["cancel_exceptions"] == 0
    assert events[2][1]["cancel_success"] == 0
    assert events[2][1]["cancel_all_instrument"] is False
    assert strategy._managed_client_order_ids == {"RESTING-1"}


def test_cancel_managed_quotes_escape_hatch_can_cancel_all_instrument_orders(
    strategy_factory,
) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)

    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert canceled_all == [str(strategy.config.maker_instrument_id)]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_instrument"] is True


def test_on_start_resets_restart_latches(strategy_factory, monkeypatch) -> None:
    maker_id = strategy_factory().config.maker_instrument_id
    strategy = strategy_factory(reference_instrument_id=maker_id)
    strategy._runtime_params_failed = True
    strategy._quote_failure_circuit_open = True
    strategy._quote_failures_ns = [1, 2]
    strategy._last_stale_cancel_ns = 123
    strategy._last_state_name = "blocked_reference_md"
    strategy._state_is_blocked = True
    strategy._last_actionable_alert_ns = {"alert": 1}
    strategy._last_actionable_alert_transition = {"alert": "old->new"}

    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._runtime_bool = lambda _name: False
    strategy._refresh_runtime_params = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    subscribed: list[str] = []
    strategy.subscribe_order_book_deltas = lambda instrument_id, **_kwargs: subscribed.append(
        str(instrument_id),
    )
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        instrument=lambda instrument_id: SimpleNamespace(
            price_precision=6,
            raw_symbol=str(instrument_id).split(".", maxsplit=1)[0],
            make_qty=lambda value: value,
        ),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)

    strategy.on_start()

    assert stopped == []
    assert strategy._runtime_params_failed is False
    assert strategy._quote_failure_circuit_open is False
    assert strategy._quote_failures_ns == []
    assert strategy._last_stale_cancel_ns == 0
    assert strategy._last_state_name is None
    assert strategy._state_is_blocked is False
    assert strategy._last_actionable_alert_ns == {}
    assert strategy._last_actionable_alert_transition == {}
    assert len(strategy._books) == 1
    assert subscribed == [str(maker_id)]


def test_on_start_allows_duplicate_instrument_ids_without_duplicate_subscriptions(
    strategy_factory,
    monkeypatch,
) -> None:
    maker_id = strategy_factory().config.maker_instrument_id
    strategy = strategy_factory(reference_instrument_id=maker_id)
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._runtime_bool = lambda _name: False
    strategy._refresh_runtime_params = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    subscribed: list[str] = []
    unsubscribed: list[str] = []
    strategy.subscribe_order_book_deltas = lambda instrument_id, **_kwargs: subscribed.append(
        str(instrument_id),
    )
    strategy.unsubscribe_order_book_deltas = lambda instrument_id, **_kwargs: unsubscribed.append(
        str(instrument_id),
    )
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        instrument=lambda instrument_id: SimpleNamespace(
            price_precision=6,
            raw_symbol=str(instrument_id).split(".", maxsplit=1)[0],
            make_qty=lambda value: value,
        ),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)

    strategy.on_start()
    strategy.on_stop()

    assert stopped == []
    assert len(strategy._books) == 1
    assert subscribed == [str(maker_id)]
    assert unsubscribed == [str(maker_id)]


def test_on_start_persists_bot_off_before_loading_runtime_params_when_enabled(
    strategy_factory,
    monkeypatch,
) -> None:
    class _ParamsManager:
        def __init__(self) -> None:
            self.stored = {"bot_on": True, "max_age_ms": 100}
            self.update_calls: list[dict[str, bool]] = []
            self.publish_calls: list[tuple[dict[str, bool], int | None]] = []

        def load(self) -> dict[str, int | bool]:
            return dict(self.stored)

        def update(self, updates: dict[str, bool]) -> dict[str, bool]:
            coerced = dict(updates)
            self.update_calls.append(coerced)
            self.stored.update(coerced)
            return coerced

        def publish_update(
            self,
            updates: dict[str, bool],
            *,
            ts_ms: int | None = None,
        ) -> dict[str, object]:
            coerced = dict(updates)
            self.publish_calls.append((coerced, ts_ms))
            return {"updates": coerced, "ts_ms": ts_ms}

    strategy = strategy_factory(force_bot_off_on_start=True)
    strategy.set_params_manager(_ParamsManager())
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy.subscribe_order_book_deltas = lambda *_args, **_kwargs: None
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        instrument=lambda instrument_id: SimpleNamespace(
            price_precision=6,
            raw_symbol=str(instrument_id).split(".", maxsplit=1)[0],
            make_qty=lambda value: value,
        ),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))

    strategy.on_start()

    manager = strategy._params_manager
    assert manager.update_calls == [{"bot_on": False}]
    assert manager.publish_calls == [({"bot_on": False}, 1_700_000_000_000)]
    assert strategy._effective_bot_on() is False


def test_on_start_preserves_runtime_bot_on_when_force_off_disabled(
    strategy_factory,
    monkeypatch,
) -> None:
    class _ParamsManager:
        def __init__(self) -> None:
            self.stored = {"bot_on": True, "max_age_ms": 100}
            self.update_calls: list[dict[str, bool]] = []
            self.publish_calls: list[tuple[dict[str, bool], int | None]] = []

        def load(self) -> dict[str, int | bool]:
            return dict(self.stored)

        def update(self, updates: dict[str, bool]) -> dict[str, bool]:
            coerced = dict(updates)
            self.update_calls.append(coerced)
            self.stored.update(coerced)
            return coerced

        def publish_update(
            self,
            updates: dict[str, bool],
            *,
            ts_ms: int | None = None,
        ) -> dict[str, object]:
            coerced = dict(updates)
            self.publish_calls.append((coerced, ts_ms))
            return {"updates": coerced, "ts_ms": ts_ms}

    strategy = strategy_factory(force_bot_off_on_start=False)
    strategy.set_params_manager(_ParamsManager())
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy.subscribe_order_book_deltas = lambda *_args, **_kwargs: None
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        instrument=lambda instrument_id: SimpleNamespace(
            price_precision=6,
            raw_symbol=str(instrument_id).split(".", maxsplit=1)[0],
            make_qty=lambda value: value,
        ),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))

    strategy.on_start()

    manager = strategy._params_manager
    assert manager.update_calls == []
    assert manager.publish_calls == []
    assert strategy._effective_bot_on() is True


def test_on_start_logs_derivative_qty_guardrail_summary(strategy_factory, monkeypatch) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
        qty_unit="base",
        qty=Decimal("1500"),
    )
    maker_instrument = _okx_linear_perpetual(maker_instrument_id)
    reference_instrument = _identity_exposure_instrument(reference_instrument_id)
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._prepare_runtime_params_for_startup = lambda: None
    strategy._refresh_runtime_params = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy.subscribe_order_book_deltas = lambda *_args, **_kwargs: None
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal("343")),
        ],
        instrument=lambda instrument_id: {
            maker_instrument_id: maker_instrument,
            reference_instrument_id: reference_instrument,
        }.get(instrument_id),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    info_messages: list[str] = []
    warning_messages: list[str] = []
    fake_log = SimpleNamespace(
        info=lambda message: info_messages.append(message),
        warning=lambda message: warning_messages.append(message),
    )
    strategy._cache = fake_cache
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    monkeypatch.setattr(type(strategy), "log", property(lambda _self: fake_log))

    strategy.on_start()

    assert stopped == []
    assert warning_messages == []
    qty_messages = [message for message in info_messages if "startup_qty_guardrail" in message]
    assert len(qty_messages) == 1
    assert "qty_unit=base" in qty_messages[0]
    assert "configured_order_qty=1500" in qty_messages[0]
    assert "resolved_order_qty_venue=150" in qty_messages[0]
    assert "local_position_qty_venue=343" in qty_messages[0]
    assert "local_position_qty_base=3430" in qty_messages[0]
    assert "conversion_status=exact_multiplier" in qty_messages[0]


def test_on_start_warns_when_derivative_position_base_conversion_is_incomplete(
    strategy_factory,
    monkeypatch,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
        qty_unit="venue",
        qty=Decimal("100"),
    )
    maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        raw_symbol="PLUME-USDT-SWAP",
        base_currency=SimpleNamespace(code="PLUME"),
        price_precision=6,
        make_qty=lambda value: Quantity.from_str(str(value)),
    )
    reference_instrument = _identity_exposure_instrument(reference_instrument_id)
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._prepare_runtime_params_for_startup = lambda: None
    strategy._refresh_runtime_params = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy.subscribe_order_book_deltas = lambda *_args, **_kwargs: None
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)
    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal("343")),
        ],
        instrument=lambda instrument_id: {
            maker_instrument_id: maker_instrument,
            reference_instrument_id: reference_instrument,
        }.get(instrument_id),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    info_messages: list[str] = []
    warning_messages: list[str] = []
    fake_log = SimpleNamespace(
        info=lambda message: info_messages.append(message),
        warning=lambda message: warning_messages.append(message),
    )
    strategy._cache = fake_cache
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    monkeypatch.setattr(type(strategy), "log", property(lambda _self: fake_log))

    strategy.on_start()

    assert stopped == []
    qty_messages = [message for message in info_messages if "startup_qty_guardrail" in message]
    assert len(qty_messages) == 1
    assert len(warning_messages) == 1
    assert "startup_qty_guardrail_missing_base" in warning_messages[0]
    assert "local_position_qty_venue=343" in warning_messages[0]
    assert "conversion_status=missing_metadata" in warning_messages[0]


def test_cancel_managed_quotes_records_cancel_all_exception_fields(strategy_factory) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)

    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)

    def _cancel_all_raises(_instrument_id: object) -> None:
        raise RuntimeError("cancel_all failed")

    strategy.cancel_all_orders = _cancel_all_raises
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_attempted"] is True
    assert events[0][1]["cancel_all_exceptions"] == 1
    assert events[0][1]["cancel_exceptions"] == 0


def test_cancel_managed_quotes_aggregates_cancel_order_exceptions_in_single_event(
    strategy_factory,
) -> None:
    strategy = strategy_factory()

    strategy._managed_orders = lambda: [
        SimpleNamespace(client_order_id="FAIL-1"),
        SimpleNamespace(client_order_id="OK-1"),
    ]
    strategy._managed_client_order_ids = {"FAIL-1", "OK-1"}

    canceled_orders: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []

    def _cancel_order(order: SimpleNamespace) -> None:
        if order.client_order_id == "FAIL-1":
            raise RuntimeError("cancel failed")
        canceled_orders.append(order.client_order_id)

    strategy.cancel_order = _cancel_order
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["OK-1"]
    assert len(events) == 1
    assert events[0][0] == "quotes_canceled"
    assert events[0][1]["cancel_attempts"] == 2
    assert events[0][1]["cancel_exceptions"] == 1
    assert events[0][1]["cancel_success"] == 1
    assert events[0][1]["cancel_all_instrument"] is False


def test_publish_state_transition_events_only_on_blocked_boundary_crossings(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2, 3, 4, 5, 6])

    transition_events: list[tuple[str, dict[str, object]]] = []
    strategy._managed_orders = list
    strategy._publish_event = lambda event, **kwargs: transition_events.append((event, kwargs))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("running")
    strategy._publish_state("blocked_maker_md")
    strategy._publish_state("blocked_reference_md")
    strategy._publish_state("blocked_reference_md")
    strategy._publish_state("running")
    strategy._publish_state("running")

    assert [name for name, _ in transition_events] == ["state_transition", "state_transition"]
    assert transition_events[0][1] == {
        "from_state": "running",
        "to_state": "blocked_maker_md",
        "from_blocked": False,
        "to_blocked": True,
    }
    assert transition_events[1][1] == {
        "from_state": "blocked_reference_md",
        "to_state": "running",
        "from_blocked": True,
        "to_blocked": False,
    }


def test_publish_state_if_due_does_not_emit_running_while_blocked(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([1, 300_000_000])

    transition_events: list[tuple[str, dict[str, object]]] = []
    strategy._managed_orders = list
    strategy._publish_event = lambda event, **kwargs: transition_events.append((event, kwargs))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("blocked_reference_md")
    strategy._publish_state_if_due()

    assert [name for name, _ in transition_events] == ["state_transition"]
    assert strategy._last_state_name == "blocked_reference_md"


def test_publish_state_resets_stale_cancel_cooldown_when_leaving_blocked(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2])

    strategy._managed_orders = list
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("blocked_reference_md")
    strategy._last_stale_cancel_ns = 123_000_000
    strategy._publish_state("running")

    assert strategy._last_stale_cancel_ns == 0


def test_compute_inventory_skew_scopes_global_and_local_inventory_by_base_and_maker_venue(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    binance_perp_id = InstrumentId.from_str("PLUMEUSDT-PERP.BINANCE")
    other_base_id = InstrumentId.from_str("BTCUSDT.BYBIT")

    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
        binance_perp_id: _identity_exposure_instrument(binance_perp_id),
        other_base_id: _identity_exposure_instrument(other_base_id, base_currency="BTC"),
    }

    positions = [
        SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(2)),
        SimpleNamespace(instrument_id=binance_perp_id, signed_qty=Decimal(-1)),
        SimpleNamespace(instrument_id=other_base_id, signed_qty=Decimal(9)),
    ]
    accounts = [
        SimpleNamespace(
            id="BYBIT-001",
            balances_total=lambda: {"PLUME": Decimal(3), "USDT": Decimal(50)},
        ),
        SimpleNamespace(
            id="BINANCE-001",
            balances_total=lambda: {"PLUME": Decimal(1000)},
        ),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda instrument_id=None: [
            position
            for position in positions
            if instrument_id is None or position.instrument_id == instrument_id
        ],
        accounts=lambda: accounts,
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(
            max_qty_global=2000,
            max_qty_local=10,
        ),
    )

    assert skew["global_position_qty"] == Decimal(1)
    assert skew["global_spot_qty"] == Decimal(1003)
    assert skew["global_inventory_qty"] == Decimal(1004)
    assert skew["global_inventory_source"] == "positions_plus_spot"
    assert skew["local_position_qty"] == Decimal(2)
    assert skew["local_spot_qty"] is None
    assert skew["local_inventory_qty"] == Decimal(2)
    assert skew["local_inventory_source"] == "positions"
    assert skew["inventory_qty"] == Decimal(1004)
    assert skew["inventory_source"] == "positions_plus_spot"


def test_compute_inventory_skew_treats_visible_maker_account_without_base_balance_as_local_zero(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: SimpleNamespace(
            id=reference_instrument_id,
            base_currency=SimpleNamespace(code="PLUME"),
        ),
    }
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda instrument_id=None: [],
        accounts=lambda: [
            SimpleNamespace(id="BYBIT-001", balances_total=lambda: {"USDT": Decimal(50)}),
            SimpleNamespace(id="BINANCE-001", balances_total=lambda: {"PLUME": Decimal(1000)}),
        ],
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(
            max_qty_global=2000,
            max_skew_bps_global=12,
            max_qty_local=10,
            max_skew_bps_local=8,
        ),
    )

    assert skew["global_position_qty"] == Decimal(0)
    assert skew["global_spot_qty"] == Decimal(1000)
    assert skew["global_inventory_qty"] == Decimal(1000)
    assert skew["local_position_qty"] is None
    assert skew["local_spot_qty"] == Decimal(0)
    assert skew["local_inventory_qty"] == Decimal(0)
    assert skew["local_ratio"] == Decimal(0)


def test_compute_inventory_skew_aggregates_visible_maker_venue_accounts_for_local_spot_inventory(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BYBIT")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: SimpleNamespace(
            id=reference_instrument_id,
            base_currency=SimpleNamespace(code="PLUME"),
        ),
    }
    empty_spot_account = SimpleNamespace(
        id="BINANCE_SPOT-SPOT-master",
        balances_total=lambda: {"PLUME": Decimal(0), "USDT": Decimal(50)},
    )
    margin_account = SimpleNamespace(
        id="BINANCE_SPOT-MARGIN-master",
        balances_total=lambda: {"PLUME": Decimal("-30143.53768988"), "USDT": Decimal("1285.28070703")},
    )
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda instrument_id=None: [],
        accounts=lambda: [empty_spot_account, margin_account],
        account_for_venue=lambda venue: empty_spot_account if str(venue) == "BINANCE_SPOT" else None,
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(
            max_qty_global=100000,
            max_skew_bps_global=25,
            max_qty_local=75000,
            max_skew_bps_local=30,
        ),
    )

    assert skew["global_position_qty"] == Decimal(0)
    assert skew["global_spot_qty"] == Decimal("-30143.53768988")
    assert skew["global_inventory_qty"] == Decimal("-30143.53768988")
    assert skew["local_position_qty"] is None
    assert skew["local_spot_qty"] == Decimal("-30143.53768988")
    assert skew["local_inventory_qty"] == Decimal("-30143.53768988")
    assert skew["local_inventory_source"] == "spot_balance"


def test_maker_local_position_summary_prefers_fresh_venue_report_over_stale_cache_net(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    stale_positions = [
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("371135"),
            avg_px_open=Decimal("0.01101"),
        ),
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("-173371"),
            avg_px_open=Decimal("0.01078"),
        ),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda instrument_id=None: (
            stale_positions
            if instrument_id is None or instrument_id == maker_instrument_id
            else []
        ),
        instrument=lambda instrument_id: (
            strategy._maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )
    strategy._last_maker_position_activity_ns = 100

    report = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_decimal_qty=Decimal("99382"),
        avg_px_open=Decimal("0.0109378"),
        ts_last=200,
        ts_init=210,
        venue_position_id=None,
    )
    strategy._handle_execution_report_message(
        SimpleNamespace(position_reports={maker_instrument_id: [report]}, ts_init=210),
    )

    summary = strategy._maker_local_position_summary("PLUME")

    assert summary.venue_qty == Decimal("99382")
    assert summary.base_qty == Decimal("99382")


def test_maker_local_position_summary_falls_back_to_cache_when_local_activity_is_newer_than_report(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    stale_positions = [
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("371135"),
            avg_px_open=Decimal("0.01101"),
        ),
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("-173371"),
            avg_px_open=Decimal("0.01078"),
        ),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        positions_open=lambda instrument_id=None: (
            stale_positions
            if instrument_id is None or instrument_id == maker_instrument_id
            else []
        ),
        instrument=lambda instrument_id: (
            strategy._maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )

    report = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_decimal_qty=Decimal("99382"),
        avg_px_open=Decimal("0.0109378"),
        ts_last=200,
        ts_init=210,
        venue_position_id=None,
    )
    strategy._handle_execution_report_message(
        SimpleNamespace(position_reports={maker_instrument_id: [report]}, ts_init=210),
    )
    strategy._last_maker_position_activity_ns = 300

    summary = strategy._maker_local_position_summary("PLUME")

    assert summary.venue_qty == Decimal("197764")
    assert summary.base_qty == Decimal("197764")


def test_timer_enforces_stale_market_data_blocks_when_feed_goes_silent(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._effective_bot_on = lambda: True
    strategy._managed_client_order_ids = {"RESTING-1"}
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 100_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 100_000_000

    canceled: list[str] = []
    states: list[str] = []
    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._cancel_managed_quotes = lambda reason, **_kwargs: canceled.append(reason)
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_actionable_alert = lambda **_kwargs: None

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert canceled == ["maker_md_stale"]
    assert states[-1] == "blocked_maker_md"


def test_timer_triggers_balances_publish_check(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._last_bot_on = False
    strategy._effective_bot_on = lambda: False

    balance_checks: list[str] = []
    strategy._publish_balances_if_due = lambda: balance_checks.append("called")

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert balance_checks == ["called"]


def test_timer_publishes_balances_when_due_after_startup_snapshot(clocked_strategy_factory) -> None:
    interval_ns = 10_000 * 1_000_000
    startup_publish_ns = 1_000_000_000
    not_due_ns = startup_publish_ns + interval_ns - 1
    due_ns = startup_publish_ns + interval_ns
    strategy = clocked_strategy_factory([not_due_ns, not_due_ns, due_ns, due_ns])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._last_bot_on = False
    strategy._effective_bot_on = lambda: False

    publish_calls: list[str] = []
    strategy._last_balances_ns = startup_publish_ns
    strategy._publish_balances = lambda: publish_calls.append("publish")
    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert publish_calls == []

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert publish_calls == ["publish"]


def test_lifecycle_handlers_reconcile_local_managed_order_state(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._managed_client_order_ids = {"A", "B", "C"}
    strategy._publish_state = lambda *_args, **_kwargs: None

    strategy.on_order_rejected(SimpleNamespace(client_order_id="A"))
    strategy.on_order_canceled(SimpleNamespace(client_order_id="B"))
    strategy.on_order_expired(SimpleNamespace(client_order_id="C"))

    assert strategy._managed_client_order_ids == set()


def test_order_rejected_enriches_event_and_alerts_after_threshold(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000, 2_000_000_000])
    strategy._runtime_params["order_reject_alert_after_count"] = 2
    strategy._runtime_params["order_reject_alert_after_s"] = Decimal(10)
    strategy._managed_client_order_ids = {"A", "B"}
    strategy._publish_actionable_alert = MakerV3Strategy._publish_actionable_alert.__get__(
        strategy,
        MakerV3Strategy,
    )

    events: list[tuple[str, dict[str, object]]] = []
    alerts: list[dict[str, object]] = []
    strategy._publish_event = lambda event, **payload: events.append((event, payload))
    strategy._publish_alert = lambda **payload: alerts.append(payload)
    strategy._publish_state = lambda *_args, **_kwargs: None

    strategy.on_order_rejected(
        SimpleNamespace(
            client_order_id="A",
            instrument_id=strategy.config.maker_instrument_id,
            reason="Parameter clOrdId error",
            due_post_only=False,
        ),
    )

    assert alerts == []
    assert events[0] == (
        "order_lifecycle",
        {
            "lifecycle": "rejected",
            "client_order_id": "A",
            "tracked_before": True,
            "tracked_after": 1,
            "instrument_id": str(strategy.config.maker_instrument_id),
            "reason": "Parameter clOrdId error",
            "due_post_only": False,
        },
    )

    strategy.on_order_rejected(
        SimpleNamespace(
            client_order_id="B",
            instrument_id=strategy.config.maker_instrument_id,
            reason="Parameter clOrdId error",
            due_post_only=False,
        ),
    )

    assert len(alerts) == 1
    assert alerts[0]["alert_key"] == "order_rejected_burst"
    assert alerts[0]["reason_code"] == "order_rejected_burst"
    assert alerts[0]["actionable"] is True
    assert alerts[0]["level"] == "error"
    assert "Parameter clOrdId error" in str(alerts[0]["message"])


def test_order_rejected_alert_is_cooldown_gated_by_reason_transition(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [
            1_000_000_000,
            2_000_000_000,
            3_000_000_000,
            4_000_000_000,
            5_000_000_000,
            6_000_000_000,
        ],
    )
    strategy._runtime_params["order_reject_alert_after_count"] = 2
    strategy._runtime_params["order_reject_alert_after_s"] = Decimal(10)
    strategy._publish_actionable_alert = MakerV3Strategy._publish_actionable_alert.__get__(
        strategy,
        MakerV3Strategy,
    )

    alerts: list[dict[str, object]] = []
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_alert = lambda **payload: alerts.append(payload)
    strategy._publish_state = lambda *_args, **_kwargs: None

    for client_order_id in ("A", "B", "C", "D"):
        strategy.on_order_rejected(
            SimpleNamespace(
                client_order_id=client_order_id,
                instrument_id=strategy.config.maker_instrument_id,
                reason="Parameter clOrdId error",
                due_post_only=False,
            ),
        )

    for client_order_id in ("E", "F"):
        strategy.on_order_rejected(
            SimpleNamespace(
                client_order_id=client_order_id,
                instrument_id=strategy.config.maker_instrument_id,
                reason="INSUFFICIENT_MARGIN",
                due_post_only=False,
            ),
        )

    assert [alert["reason_code"] for alert in alerts] == [
        "order_rejected_burst",
        "order_rejected_burst",
    ]
    assert "Parameter clOrdId error" in str(alerts[0]["message"])
    assert "INSUFFICIENT_MARGIN" in str(alerts[1]["message"])


def test_order_filled_reconciles_managed_tracking_without_cache_closed(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._managed_client_order_ids = {"A"}

    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_event = lambda name, **payload: published.append((name, payload))
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None

    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            client_order_id="A",
            trade_id="T1",
            order_side="BUY",
            last_qty=Decimal(1),
            last_px=Decimal(100),
            ts_event=123,
        ),
    )

    assert strategy._managed_client_order_ids == set()
    assert (
        "order_lifecycle",
        {"lifecycle": "filled", "client_order_id": "A", "tracked_before": True, "tracked_after": 0},
    ) in published


def test_quote_failure_circuit_breaker_triggers_stop(strategy_factory) -> None:
    strategy = strategy_factory()

    canceled: list[tuple[str, bool]] = []
    states: list[str] = []
    stopped: list[bool] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: canceled.append(
        (reason, force),
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy.stop_immediately = lambda: stopped.append(True)

    strategy._handle_quote_failure(now_ns=1_000_000_000, exc=RuntimeError("boom-1"), context="test")
    strategy._handle_quote_failure(now_ns=2_000_000_000, exc=RuntimeError("boom-2"), context="test")

    assert stopped == [True]
    assert canceled[-1] == ("quote_fail_circuit_breaker", True)
    assert states[-1] == "blocked_quote_failures"


def test_quote_failure_circuit_breaker_stops_even_if_side_effects_raise(
    strategy_factory,
    raise_runtime_error,
) -> None:
    strategy = strategy_factory()

    strategy._runtime_params["quote_fail_critical_after_count"] = 1
    strategy._publish_event = raise_runtime_error
    strategy._publish_alert = raise_runtime_error
    strategy._publish_state = raise_runtime_error
    strategy._cancel_managed_quotes = raise_runtime_error

    stopped: list[bool] = []
    strategy.stop_immediately = lambda: stopped.append(True)

    strategy._handle_quote_failure(now_ns=1_000_000_000, exc=RuntimeError("boom"), context="test")

    assert strategy._quote_failure_circuit_open is True
    assert stopped == [True]


def test_on_stop_during_startup_cleanup_keeps_managed_only_cancel_scope(strategy_factory) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)
    strategy._startup_cleanup_pending = True
    strategy._stop_allow_instrument_cancel_override = False

    cancel_calls: list[tuple[str, bool, bool | None]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **kwargs: cancel_calls.append(
        (reason, force, kwargs.get("allow_instrument_cancel")),
    )
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None

    strategy.on_stop()

    assert cancel_calls == [("on_stop", True, False)]
    assert strategy._startup_cleanup_pending is False
    assert strategy._stop_allow_instrument_cancel_override is None


def test_on_stop_preserves_tracked_ids_without_cancel_all_by_default(strategy_factory) -> None:
    strategy = strategy_factory()

    strategy._managed_client_order_ids = {"RESTING-1"}
    strategy._managed_orders = list

    canceled_all: list[str] = []
    states: list[str] = []
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_state = lambda state: states.append(state)

    strategy.on_stop()
    strategy.on_stop()

    assert canceled_all == []
    assert strategy._managed_client_order_ids == {"RESTING-1"}
    assert states == ["on_stop", "on_stop"]


def test_timer_triggers_fallback_requote_when_books_are_fresh_and_quiet(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy._effective_bot_on = lambda: True
    strategy._last_bot_on = True
    strategy._enforce_stale_market_data = lambda **_kwargs: None
    strategy._quote_failure_circuit_open = False
    strategy.INTERNAL_REQUOTE_THROTTLE_MS = 100
    strategy._last_requote_ns = 0
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 450_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 450_000_000

    refreshes: list[int] = []
    strategy._refresh_quotes = lambda now_ns, *, quote_cycle_id=None: refreshes.append(now_ns)

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert refreshes == [500_000_000]


def test_timer_skips_quote_management_while_market_exit_is_active(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy._effective_bot_on = lambda: True
    strategy._last_bot_on = True
    strategy.is_exiting = lambda: True

    stale_checks: list[int] = []
    refreshes: list[int] = []
    strategy._enforce_stale_market_data = lambda *, now_ns: stale_checks.append(now_ns)
    strategy._refresh_quotes = lambda now_ns, *, quote_cycle_id=None: refreshes.append(now_ns)

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert stale_checks == []
    assert refreshes == []


def test_cancel_managed_quotes_honors_cancel_all_escape_hatch_without_local_state(
    strategy_factory,
) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)
    strategy._managed_orders = list
    strategy._managed_client_order_ids = set()

    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_all == [str(strategy.config.maker_instrument_id)]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_instrument"] is True
    assert events[0][1]["cancel_attempts"] == 0
    assert events[0][1]["tracked_count"] == 0


def test_compute_inventory_skew_splits_global_and_local_inventory_by_base_and_maker_venue(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)

    bybit_position = SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(2))
    binance_position = SimpleNamespace(
        instrument_id=reference_instrument_id,
        signed_qty=Decimal(-1),
    )
    other_asset_instrument_id = InstrumentId.from_str("BTCUSDT.BYBIT")
    other_asset_position = SimpleNamespace(
        instrument_id=other_asset_instrument_id,
        signed_qty=Decimal(9),
    )

    bybit_account = SimpleNamespace(
        balances_total=lambda: {
            "PLUME": Decimal(3),
            "BTC": Decimal(7),
        },
    )
    binance_account = SimpleNamespace(
        balances_total=lambda: {
            "PLUME": Decimal(5),
        },
    )
    instrument_by_id = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
        other_asset_instrument_id: _identity_exposure_instrument(
            other_asset_instrument_id,
            base_currency="BTC",
        ),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda instrument_id=None: (
            [bybit_position, binance_position, other_asset_position]
            if instrument_id is None
            else []
        ),
        instrument=lambda instrument_id: instrument_by_id.get(instrument_id),
        accounts=lambda: [bybit_account, binance_account],
        account_for_venue=lambda venue: bybit_account if str(venue) == "BYBIT" else None,
    )

    skew = strategy._compute_inventory_skew(
        runtime_params={
            "des_qty_global": Decimal(0),
            "max_qty_global": Decimal(10),
            "max_skew_bps_global": Decimal(20),
            "des_qty_local": Decimal(0),
            "max_qty_local": Decimal(10),
            "max_skew_bps_local": Decimal(10),
            "linear_offset_bps": Decimal(0),
        },
    )

    assert skew["global_position_qty"] == Decimal(1)
    assert skew["global_spot_qty"] == Decimal(8)
    assert skew["global_inventory_qty"] == Decimal(9)
    assert skew["local_position_qty"] is None
    assert skew["local_spot_qty"] == Decimal(3)
    assert skew["local_inventory_qty"] == Decimal(3)
    assert skew["inventory_qty"] == Decimal(9)
    assert skew["global_ratio"] == Decimal("0.9")
    assert skew["local_ratio"] == Decimal("0.3")


def test_compute_inventory_skew_uses_shared_portfolio_global_qty_and_maker_leg_local_only(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    strategy = clocked_strategy_factory(
        [1_500_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(36689)),
            SimpleNamespace(
                instrument_id=InstrumentId.from_str("PLUME-USDT-SWAP.OKX"),
                signed_qty=Decimal(-9806),
            ),
        ],
        accounts=lambda: [
            SimpleNamespace(id="BYBIT-001", balances_total=lambda: {"PLUME": Decimal("5434.3519")}),
            SimpleNamespace(id="BINANCE_SPOT-001", balances_total=lambda: {"PLUME": Decimal(100)}),
        ],
        account_for_venue=lambda venue: (
            SimpleNamespace(id="BYBIT-001", balances_total=lambda: {"PLUME": Decimal("5434.3519")})
            if str(venue) == "BYBIT"
            else None
        ),
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    fake_redis = _FakeRedis()
    aggregate_key = FluxRedisKeys.portfolio_inventory(
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    fake_redis.set(
        aggregate_key,
        encode_portfolio_inventory(
            {
                "portfolio_id": "tokenmm",
                "base_currency": "PLUME",
                "global_qty_base": "32317.3519",
                "global_qty": "32317.3519",
                "ts_ms": 1_000,
                "stale_after_ms": 3_000,
                "components": [],
                "missing_required": [],
                "degraded": False,
            },
        ),
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(max_qty_global=50_000),
    )

    assert skew["global_inventory_qty"] == Decimal("32317.3519")
    assert skew["global_inventory_qty_base"] == Decimal("32317.3519")
    assert skew["global_inventory_source"] == "portfolio_component_sum"
    assert skew["local_position_qty"] == Decimal(36689)
    assert skew["local_position_qty_base"] == Decimal(36689)
    assert skew["local_spot_qty"] is None
    assert skew["local_inventory_qty"] == Decimal(36689)
    assert skew["local_inventory_qty_base"] == Decimal(36689)


def test_compute_inventory_skew_uses_partial_shared_portfolio_global_qty_when_enabled(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    strategy = clocked_strategy_factory(
        [1_500_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(36689)),
        ],
        accounts=lambda: [],
        account_for_venue=lambda venue: None,
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    fake_redis = _FakeRedis()
    aggregate_key = FluxRedisKeys.portfolio_inventory(
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    fake_redis.set(
        aggregate_key,
        encode_portfolio_inventory(
            {
                "portfolio_id": "tokenmm",
                "base_currency": "PLUME",
                "global_qty_base": "129016.69578451",
                "global_qty": "129016.69578451",
                "aggregation_mode": "partial",
                "global_qty_base_complete": False,
                "global_qty_complete": False,
                "ts_ms": 1_000,
                "stale_after_ms": 3_000,
                "components": [],
                "missing_required": ["strategy_02"],
                "stale_required": [],
                "null_qty_required": [],
                "degraded": True,
            },
        ),
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
        allow_partial_global_risk=True,
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(max_qty_global=250_000),
    )

    assert skew["global_inventory_qty"] == Decimal("129016.69578451")
    assert skew["global_inventory_qty_base"] == Decimal("129016.69578451")
    assert skew["global_inventory_source"] == "portfolio_component_partial_sum"
    assert skew["global_inventory_qty_base_complete"] is False
    assert skew["global_inventory_qty_complete"] is False
    assert skew["global_inventory_aggregation_mode"] == "partial"
    assert skew["global_inventory_missing_required"] == ["strategy_02"]


def test_compute_inventory_skew_uses_base_exposure_for_derivative_positions(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _okx_linear_perpetual(maker_instrument_id)
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: SimpleNamespace(
            base_currency=SimpleNamespace(code="PLUME"),
            id=reference_instrument_id,
        ),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda instrument_id=None: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(343)),
        ],
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        accounts=lambda: [],
        account_for_venue=lambda venue: None,
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(max_qty_global=5_000, max_qty_local=5_000),
    )

    assert skew["global_position_qty_venue"] == Decimal(343)
    assert skew["global_position_qty_base"] == Decimal(3430)
    assert skew["global_position_qty_complete"] is True
    assert skew["global_position_qty_conversion_status"] == "exact_multiplier"
    assert (
        skew["global_position_qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )
    assert skew["global_position_qty"] == Decimal(3430)
    assert skew["global_inventory_qty_base"] == Decimal(3430)
    assert skew["local_position_qty_venue"] == Decimal(343)
    assert skew["local_position_qty_base"] == Decimal(3430)
    assert skew["local_position_qty_complete"] is True
    assert skew["local_position_qty_conversion_status"] == "exact_multiplier"
    assert (
        skew["local_position_qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )
    assert skew["local_position_qty"] == Decimal(3430)
    assert skew["local_inventory_qty_base"] == Decimal(3430)
    assert skew["local_inventory_qty"] == Decimal(3430)


def test_compute_inventory_skew_degrades_when_position_base_exposure_is_unavailable(
    strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    strategy = strategy_factory(
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda instrument_id=None: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(343)),
        ],
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        accounts=lambda: [
            SimpleNamespace(id="BINANCE-001", balances_total=lambda: {"PLUME": Decimal(1000)}),
        ],
        account_for_venue=lambda venue: None,
    )

    skew = strategy._compute_inventory_skew(
        runtime_params=_inventory_runtime_params(
            max_qty_global=2_000,
            max_skew_bps_global=12,
            max_qty_local=500,
            max_skew_bps_local=8,
        ),
    )

    assert skew["global_position_qty_venue"] == Decimal(343)
    assert skew["global_position_qty_base"] is None
    assert skew["global_position_qty_conversion_status"] == "missing_metadata"
    assert skew["global_inventory_qty_base"] is None
    assert skew["global_inventory_qty"] is None
    assert skew["global_inventory_source"] == "position_exposure_unavailable"
    assert skew["global_spot_qty"] == Decimal(1000)
    assert skew["global_ratio"] is None
    assert skew["global_skew_bps"] is None
    assert skew["local_position_qty_venue"] == Decimal(343)
    assert skew["local_position_qty_base"] is None
    assert skew["local_position_qty_conversion_status"] == "missing_metadata"
    assert skew["local_inventory_qty_base"] is None
    assert skew["local_inventory_qty"] is None
    assert skew["local_inventory_source"] == "position_exposure_unavailable"
    assert skew["local_ratio"] is None
    assert skew["local_skew_bps"] is None


def test_publish_portfolio_inventory_component_uses_maker_leg_only(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    strategy = clocked_strategy_factory(
        [2_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = _identity_exposure_instrument(maker_instrument_id)
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(36689)),
        ],
        accounts=lambda: [
            SimpleNamespace(id="BYBIT-001", balances_total=lambda: {"PLUME": Decimal("5434.3519")}),
        ],
        account_for_venue=lambda venue: (
            SimpleNamespace(id="BYBIT-001", balances_total=lambda: {"PLUME": Decimal("5434.3519")})
            if str(venue) == "BYBIT"
            else None
        ),
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    fake_redis = _FakeRedis()
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
    )

    strategy._publish_portfolio_inventory_component(state="running")

    key = FluxRedisKeys.portfolio_inventory_component(
        strategy_id=strategy._external_strategy_id,
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    component = decode_component(fake_redis.get(key))

    assert component is not None
    assert component.local_qty_base == Decimal(36689)
    assert component.local_position_qty_venue == Decimal(36689)
    assert component.local_position_qty_base == Decimal(36689)
    assert component.qty_conversion_status == "identity"
    assert component.qty_conversion_source == "instrument.info:base_exposure_mode=identity"
    assert component.maker_instrument_id == "PLUMEUSDT-LINEAR.BYBIT"


def test_publish_portfolio_inventory_component_keeps_conversion_diagnostics_for_unavailable_base_exposure(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    strategy = clocked_strategy_factory(
        [2_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: _identity_exposure_instrument(reference_instrument_id),
    }
    strategy._cache = SimpleNamespace(
        positions_open=lambda: [
            SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(343)),
        ],
        accounts=lambda: [],
        account_for_venue=lambda venue: None,
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    fake_redis = _FakeRedis()
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
    )

    strategy._publish_portfolio_inventory_component(state="running")

    key = FluxRedisKeys.portfolio_inventory_component(
        strategy_id=strategy._external_strategy_id,
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    component = decode_component(fake_redis.get(key))

    assert component is not None
    assert component.local_qty_base is None
    assert component.local_position_qty_venue == Decimal(343)
    assert component.local_position_qty_base is None
    assert component.qty_conversion_status == "missing_metadata"
    assert component.qty_conversion_source == "generic:instrument multiplier unavailable"
