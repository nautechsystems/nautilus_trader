from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from flux.runners.tg_bots import run_lan_rogue_trader_alert as runner


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[5]


def test_module_invocation_requires_config_arg() -> None:
    repo_root = _repo_root()
    env = dict(os.environ)
    env["PYTHONPATH"] = str(repo_root)

    result = subprocess.run(  # noqa: S603 - controlled test invocation of the repo runner module
        [sys.executable, "-m", "nautilus_trader.flux.runners.tg_bots.run_lan_rogue_trader_alert"],
        cwd=repo_root,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 2
    assert "--config" in result.stderr


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
            if url.endswith("/papi/v1/balance"):
                return FakeResponse(
                    200,
                    {
                        "asset": "USDT",
                        "totalWalletBalance": "123.45",
                    },
                )
            return FakeResponse(
                200,
                {
                    "accountType": "SPOT",
                    "balances": [
                        {"asset": "USDT", "free": "0.00", "locked": "0.00"},
                    ],
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
    assert len(session.get_calls) == 2
    state_payload = json.loads((tmp_path / "state" / "lan_rogue_trader_alert.json").read_text())
    assert state_payload["last_balance"] == "123.45"
