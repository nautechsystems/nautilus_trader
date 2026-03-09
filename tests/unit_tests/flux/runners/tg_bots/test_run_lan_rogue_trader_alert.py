from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

from flux.runners.tg_bots import run_lan_rogue_trader_alert as runner


def test_once_mode_returns_zero(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "lan_rogue_trader_alert.ini"
    service_calls: list[str] = []

    class DummySession:
        pass

    class DummyService:
        def poll_once(self) -> None:
            service_calls.append("poll_once")

        def run_forever(self) -> None:
            service_calls.append("run_forever")

    def fake_load_config(path: Path) -> object:
        assert path == config_path
        return SimpleNamespace(
            binance_base_url="https://papi.binance.com",
            asset="USDT",
            binance_api_key="k",
            binance_api_secret="s",
            telegram_bot_token="t",
            telegram_chat_id=-100123,
            telegram_thread_id=42,
            strict_thread=False,
            state_path=tmp_path / "state.json",
        )

    logger = SimpleNamespace(error=lambda *args, **kwargs: None, exception=lambda *args, **kwargs: None)

    monkeypatch.setattr(runner, "load_config", fake_load_config)
    monkeypatch.setattr(runner.requests, "Session", lambda: DummySession())
    monkeypatch.setattr(runner, "BinancePmClient", lambda **_: object())
    monkeypatch.setattr(runner, "TelegramNotifier", lambda **_: object())
    monkeypatch.setattr(runner, "JsonStateStore", lambda path: object())
    monkeypatch.setattr(runner, "LanRogueTraderAlertService", lambda *args, **kwargs: DummyService())
    monkeypatch.setattr(runner, "configure_service_logging", lambda **_: logger)

    exit_code = runner.main(["--config", str(config_path), "--once"])

    assert exit_code == 0
    assert service_calls == ["poll_once"]
