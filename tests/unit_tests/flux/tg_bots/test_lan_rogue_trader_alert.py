from __future__ import annotations

from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest

from flux.tg_bots.lan_rogue_trader_alert import BinancePmClient
from flux.tg_bots.lan_rogue_trader_alert import JsonStateStore
from flux.tg_bots.lan_rogue_trader_alert import LanRogueTraderAlertService
from flux.tg_bots.lan_rogue_trader_alert import MissingAssetError
from flux.tg_bots.lan_rogue_trader_alert import TelegramNotifier
from flux.tg_bots.lan_rogue_trader_alert import WatchConfig
from flux.tg_bots.lan_rogue_trader_alert import WatchState


pytestmark = pytest.mark.unit


class DummyNotifier:
    def __init__(self) -> None:
        self.messages: list[str] = []

    def send_message(self, text: str) -> bool:
        self.messages.append(text)
        return True


class DummyBinance:
    def __init__(self, sequence: list[Any]) -> None:
        self.sequence = list(sequence)

    def fetch_balance(self) -> Decimal:
        if not self.sequence:
            raise RuntimeError("no more values")
        item = self.sequence.pop(0)
        if isinstance(item, Exception):
            raise item
        return item


class FakeResponse:
    def __init__(self, status_code: int, payload: dict[str, Any] | None = None) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""

    def json(self) -> Any:
        if self._payload is None:
            raise ValueError("no payload")
        return self._payload


class FakeSession:
    def __init__(self, responses: list[FakeResponse]) -> None:
        self.responses = list(responses)
        self.calls: list[dict[str, Any]] = []

    def post(self, url: str, json: dict[str, Any], timeout: float) -> FakeResponse:  # noqa: A002
        self.calls.append({"url": url, "json": dict(json), "timeout": timeout})
        if not self.responses:
            raise RuntimeError("missing response")
        return self.responses.pop(0)


class FakeGetResponse:
    def __init__(self, status_code: int, payload: Any) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""

    def json(self) -> Any:
        return self._payload


class FakeGetSession:
    def __init__(self, response: FakeGetResponse) -> None:
        self._response = response
        self.calls: list[dict[str, Any]] = []

    def get(self, url: str, params: Any, headers: dict[str, str], timeout: float) -> FakeGetResponse:
        self.calls.append({"url": url, "params": list(params), "headers": dict(headers), "timeout": timeout})
        return self._response


def make_config(tmp_path: Path, **overrides: Any) -> WatchConfig:
    cfg = dict(
        poll_secs=60,
        cooldown_secs=3600,
        binance_base_url="https://papi.binance.com",
        asset="USDT",
        binance_api_key="k",
        binance_api_secret="s",
        account_label="LanSub: traderX",
        telegram_bot_token="t",
        telegram_chat_id=-100123,
        telegram_thread_id=42,
        strict_thread=False,
        state_path=tmp_path / "lan_state.json",
        emergency_bypass_usdt=Decimal("0"),
        timezone_name="Asia/Bangkok",
        send_baseline=False,
    )
    cfg.update(overrides)
    return WatchConfig(**cfg)


def test_cooldown_suppresses_and_summary_fires(tmp_path: Path) -> None:
    cfg = make_config(tmp_path, cooldown_secs=3600)
    notifier = DummyNotifier()
    store = JsonStateStore(cfg.state_path)
    svc = LanRogueTraderAlertService(cfg, DummyBinance([]), notifier, store)

    state = WatchState.initial(Decimal("100"))
    state.last_alert_at = 1000
    state.last_alert_balance = Decimal("100")
    svc.state = state

    svc._apply_balance(balance=Decimal("99"), now=1100)
    assert notifier.messages == []
    assert state.pending is True
    assert state.pending_count == 1

    svc._apply_balance(balance=Decimal("99"), now=4601)
    assert len(notifier.messages) == 1
    assert "summary" in notifier.messages[0].lower()
    assert state.pending is False
    assert state.last_alert_at == 4601
    assert state.last_alert_balance == Decimal("99")


def test_immediate_alert_fires_after_cooldown_elapsed(tmp_path: Path) -> None:
    cfg = make_config(tmp_path, cooldown_secs=300)
    notifier = DummyNotifier()
    store = JsonStateStore(cfg.state_path)
    svc = LanRogueTraderAlertService(cfg, DummyBinance([]), notifier, store)

    state = WatchState.initial(Decimal("100"))
    state.last_alert_at = 1000
    state.last_alert_balance = Decimal("100")
    svc.state = state

    svc._apply_balance(balance=Decimal("101"), now=1401)

    assert len(notifier.messages) == 1
    assert "USDT balance changed" in notifier.messages[0]
    assert state.pending is False
    assert state.last_alert_at == 1401
    assert state.last_alert_balance == Decimal("101")


def test_emergency_bypass_fires_inside_cooldown(tmp_path: Path) -> None:
    cfg = make_config(tmp_path, cooldown_secs=3600, emergency_bypass_usdt=Decimal("10"))
    notifier = DummyNotifier()
    store = JsonStateStore(cfg.state_path)
    svc = LanRogueTraderAlertService(cfg, DummyBinance([]), notifier, store)

    state = WatchState.initial(Decimal("100"))
    state.last_alert_at = 1000
    state.last_alert_balance = Decimal("100")
    svc.state = state

    svc._apply_balance(balance=Decimal("85"), now=1200)

    assert len(notifier.messages) == 1
    assert "USDT balance changed" in notifier.messages[0]
    assert state.pending is False


def test_telegram_thread_fallback_to_root_when_not_strict() -> None:
    session = FakeSession(
        [
            FakeResponse(
                400,
                {
                    "ok": False,
                    "error_code": 400,
                    "description": "Bad Request: message thread not found",
                },
            ),
            FakeResponse(200, {"ok": True, "result": {"message_id": 1}}),
        ]
    )
    notifier = TelegramNotifier(
        bot_token="token",
        chat_id=-100123,
        thread_id=42,
        strict_thread=False,
        session=session,  # type: ignore[arg-type]
        max_retries=1,
    )

    ok = notifier.send_message("hello")
    assert ok is True
    assert len(session.calls) == 2
    assert session.calls[0]["json"]["message_thread_id"] == 42
    assert "message_thread_id" not in session.calls[1]["json"]
    assert "WARNING: thread_id failed" in session.calls[1]["json"]["text"]


def test_missing_usdt_row_alerts_once_per_episode(tmp_path: Path) -> None:
    cfg = make_config(tmp_path, send_baseline=False)
    notifier = DummyNotifier()
    store = JsonStateStore(cfg.state_path)
    binance = DummyBinance(
        [
            MissingAssetError("Asset USDT missing in Binance PM balance payload"),
            MissingAssetError("Asset USDT missing in Binance PM balance payload"),
            Decimal("100"),
            MissingAssetError("Asset USDT missing in Binance PM balance payload"),
        ]
    )
    svc = LanRogueTraderAlertService(cfg, binance, notifier, store)

    svc.poll_once()
    svc.poll_once()
    svc.poll_once()
    svc.poll_once()

    error_msgs = [message for message in notifier.messages if "watch error" in message.lower()]
    assert len(error_msgs) == 2


def test_binance_signature_is_deterministic() -> None:
    params = [
        ("asset", "USDT"),
        ("timestamp", "1700000000000"),
        ("recvWindow", "5000"),
    ]
    signature = BinancePmClient.sign_params(params=params, secret="testsecret")
    assert signature == "5ce2e9906da7a0647c0769e04c4335e224dff5553b7954030d102d9372c106d9"

