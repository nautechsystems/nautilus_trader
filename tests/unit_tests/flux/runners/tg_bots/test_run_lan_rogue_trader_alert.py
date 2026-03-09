from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from flux.runners.tg_bots import run_lan_rogue_trader_alert as runner


def test_once_mode_returns_zero(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "lan_rogue_trader_alert.ini"
    config_path.write_text(
        """[lan_rogue_trader_alert]
poll_secs = 60
cooldown_secs = 300
binance_base_url = https://papi.binance.com
asset = USDT
api_key_env = BINANCE_API_KEY
api_secret_env = BINANCE_API_SECRET
account_label = LanSub: traderX
telegram_bot_token_env = TELEGRAM_BOT_TOKEN
telegram_chat_id = -100123
send_baseline = false
""",
        encoding="utf-8",
    )

    class FakeResponse:
        def __init__(self, status_code: int, payload: Any) -> None:
            self.status_code = status_code
            self._payload = payload
            self.text = ""

        def json(self) -> Any:
            return self._payload

    class FakeSession:
        def __init__(self) -> None:
            self.get_calls: list[dict[str, Any]] = []

        def get(self, url: str, params: Any, headers: dict[str, str], timeout: float) -> FakeResponse:
            self.get_calls.append(
                {"url": url, "params": list(params), "headers": dict(headers), "timeout": timeout}
            )
            return FakeResponse(
                200,
                {
                    "asset": "USDT",
                    "totalWalletBalance": "123.45",
                },
            )

        def close(self) -> None:
            return None

    session = FakeSession()
    logger = SimpleNamespace(
        error=lambda *args, **kwargs: None,
        exception=lambda *args, **kwargs: None,
        info=lambda *args, **kwargs: None,
    )

    monkeypatch.setenv("BINANCE_API_KEY", "k")
    monkeypatch.setenv("BINANCE_API_SECRET", "s")
    monkeypatch.setenv("TELEGRAM_BOT_TOKEN", "t")
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(runner, "build_http_session", lambda: session)
    monkeypatch.setattr(runner, "configure_service_logging", lambda **_: logger)

    exit_code = runner.main(["--config", str(config_path), "--once"])

    assert exit_code == 0
    assert len(session.get_calls) == 1
    state_payload = json.loads((tmp_path / "state" / "lan_rogue_trader_alert.json").read_text())
    assert state_payload["last_balance"] == "123.45"
